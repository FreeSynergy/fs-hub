//! The private RPC thread: owns the capnp-rpc connection, answers commands from [`HubSession`].

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use capnp_rpc::{RpcSystem, new_client, rpc_twoparty_capnp::Side, twoparty};
use fs_hub_proto::common_capnp::{change, change_listener};
use fs_hub_proto::hub_capnp::{hub, session};
use fs_hub_proto::{
    ChangeNotice, Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome, read_pull_batch,
    read_program_info, read_proposal, read_push_outcome, read_uuid, write_activities, write_activity_id, write_manifest,
};
use fs_store::{Activity, ActivityId, ObjectId};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::{Error, HubSession, Result};

type Reply<T> = oneshot::Sender<Result<T>>;

pub(crate) enum Cmd {
    Push(Vec<Activity>, Reply<PushOutcome>),
    Pull(Option<ActivityId>, u32, Reply<PullBatch>),
    Ack(ActivityId, Reply<()>),
    Programs(Reply<Vec<ProgramInfo>>),
    Proposals(Reply<Vec<Proposal>>),
    Subscribe(Reply<mpsc::UnboundedReceiver<ChangeNotice>>),
}

/// Connect and register on a private thread; returns the session handle.
pub(crate) async fn start(socket: PathBuf, manifest: Manifest) -> Result<HubSession> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = oneshot::channel::<Result<()>>();
    std::thread::Builder::new().name("fs-hub-client".into()).spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = ready_tx.send(Err(e.into()));
                return;
            }
        };
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, run(socket, manifest, cmd_rx, ready_tx));
    })?;
    ready_rx.await.map_err(|_| Error::Closed)??;
    Ok(HubSession::new(cmd_tx))
}

async fn run(socket: PathBuf, manifest: Manifest, mut cmds: mpsc::UnboundedReceiver<Cmd>, ready: oneshot::Sender<Result<()>>) {
    let stream = match UnixStream::connect(&socket).await {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => {
            let _ = ready.send(Err(Error::NoHub));
            return;
        }
        Err(e) => {
            let _ = ready.send(Err(e.into()));
            return;
        }
    };
    let (r, w) = stream.into_split();
    let network = twoparty::VatNetwork::new(r.compat(), w.compat_write(), Side::Client, Default::default());
    let mut rpc = RpcSystem::new(Box::new(network), None);
    let hub: hub::Client = rpc.bootstrap(Side::Server);
    let rpc_task = tokio::task::spawn_local(rpc);

    let session = match register(&hub, &manifest).await {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let subscriptions: RefCell<Vec<fs_hub_proto::common_capnp::subscription::Client>> = RefCell::new(vec![]);

    while let Some(cmd) = cmds.recv().await {
        match cmd {
            Cmd::Push(acts, reply) => {
                let _ = reply.send(push(&session, &acts).await);
            }
            Cmd::Pull(since, limit, reply) => {
                let _ = reply.send(pull(&session, since, limit).await);
            }
            Cmd::Ack(id, reply) => {
                let _ = reply.send(ack(&session, id).await);
            }
            Cmd::Programs(reply) => {
                let _ = reply.send(programs(&hub).await);
            }
            Cmd::Proposals(reply) => {
                let _ = reply.send(proposals(&hub).await);
            }
            Cmd::Subscribe(reply) => {
                let (tx, rx) = mpsc::unbounded_channel();
                let r = subscribe(&session, tx).await.map(|sub| {
                    subscriptions.borrow_mut().push(sub);
                    rx
                });
                let _ = reply.send(r);
            }
        }
    }
    // Session dropped: closing the connection ends the hub-side session.
    drop(subscriptions);
    drop(session);
    drop(hub);
    rpc_task.abort();
}

async fn register(hub: &hub::Client, manifest: &Manifest) -> Result<session::Client> {
    let mut req = hub.register_request();
    write_manifest(req.get().init_manifest(), manifest);
    let resp = req.send().promise.await?;
    Ok(resp.get()?.get_session()?)
}

async fn push(s: &session::Client, acts: &[Activity]) -> Result<PushOutcome> {
    let mut req = s.push_request();
    write_activities(req.get().init_activities(acts.len() as u32), acts)?;
    let resp = req.send().promise.await?;
    Ok(read_push_outcome(resp.get()?)?)
}

async fn pull(s: &session::Client, since: Option<ActivityId>, limit: u32) -> Result<PullBatch> {
    let mut req = s.pull_request();
    {
        let mut p = req.get();
        write_activity_id(p.reborrow().init_since(), since);
        p.set_limit(limit);
    }
    let resp = req.send().promise.await?;
    Ok(read_pull_batch(resp.get()?)?)
}

async fn ack(s: &session::Client, id: ActivityId) -> Result<()> {
    let mut req = s.ack_request();
    write_activity_id(req.get().init_up_to(), Some(id));
    req.send().promise.await?;
    Ok(())
}

async fn programs(hub: &hub::Client) -> Result<Vec<ProgramInfo>> {
    let resp = hub.programs_request().send().promise.await?;
    Ok(resp.get()?.get_programs()?.iter().map(read_program_info).collect::<std::result::Result<_, _>>()?)
}

async fn proposals(hub: &hub::Client) -> Result<Vec<Proposal>> {
    let resp = hub.proposals_request().send().promise.await?;
    Ok(resp.get()?.get_proposals()?.iter().map(read_proposal).collect::<std::result::Result<_, _>>()?)
}

struct Listener {
    tx: mpsc::UnboundedSender<ChangeNotice>,
}

impl change_listener::Server for Listener {
    async fn change(self: Rc<Self>, params: change_listener::ChangeParams) -> std::result::Result<(), capnp::Error> {
        let notice = match params.get()?.get_change()?.which()? {
            change::Which::Created(e) => {
                let e = e?;
                ChangeNotice::Created {
                    id: object_id(e.get_id()?)?,
                    object_type: e.get_type()?.to_string()?,
                    name: e.get_name()?.to_string()?,
                }
            }
            change::Which::Updated(e) => {
                let e = e?;
                ChangeNotice::Updated {
                    id: object_id(e.get_id()?)?,
                    object_type: e.get_type()?.to_string()?,
                    name: e.get_name()?.to_string()?,
                }
            }
            change::Which::Deleted(u) => ChangeNotice::Deleted { id: object_id(u?)? },
        };
        let _ = self.tx.send(notice);
        Ok(())
    }
}

fn object_id(u: fs_hub_proto::common_capnp::uuid::Reader<'_>) -> std::result::Result<ObjectId, capnp::Error> {
    let u = read_uuid(u).ok_or_else(|| capnp::Error::failed("change without id".into()))?;
    ObjectId::from_uuid(u).map_err(|e| capnp::Error::failed(e.to_string()))
}

async fn subscribe(
    s: &session::Client,
    tx: mpsc::UnboundedSender<ChangeNotice>,
) -> Result<fs_hub_proto::common_capnp::subscription::Client> {
    let mut req = s.subscribe_request();
    req.get().set_listener(new_client(Listener { tx }));
    let resp = req.send().promise.await?;
    Ok(resp.get()?.get_subscription()?)
}
