//! The Cap'n Proto RPC layer (`schema/hub.capnp`) over the [`HubCore`]. Runs on a `LocalSet`
//! (capnp-rpc is not `Send`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use capnp_rpc::{RpcSystem, new_client, rpc_twoparty_capnp::Side, twoparty};
use fs_hub_proto::common_capnp::{change_listener, subscription};
use fs_hub_proto::hub_capnp::{hub, session};
use fs_hub_proto::{
    read_activity_id, read_manifest, write_activities, write_activity_id, write_envelope, write_program_info,
    write_proposal, write_push_outcome, read_activities, CONTRACT_VERSION,
};
use fs_store::{Activity, ActivityKind};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::core::{HubCore, LiveGuard};
use crate::{Error, Result};

fn rpc_err(e: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(e.to_string())
}

struct HubImpl {
    core: Arc<HubCore>,
}

impl hub::Server for HubImpl {
    async fn register(
        self: Rc<Self>,
        params: hub::RegisterParams,
        mut results: hub::RegisterResults,
    ) -> std::result::Result<(), capnp::Error> {
        let manifest = read_manifest(params.get()?.get_manifest()?).map_err(rpc_err)?;
        let program = manifest.program.clone();
        let guard = self.core.register(manifest).await.map_err(rpc_err)?;
        let session = SessionImpl { core: self.core.clone(), program, _live: guard };
        let mut r = results.get();
        r.set_session(new_client(session));
        r.set_hub_version(CONTRACT_VERSION);
        Ok(())
    }

    async fn programs(
        self: Rc<Self>,
        _: hub::ProgramsParams,
        mut results: hub::ProgramsResults,
    ) -> std::result::Result<(), capnp::Error> {
        let list = self.core.programs().await;
        let mut out = results.get().init_programs(list.len() as u32);
        for (i, p) in list.iter().enumerate() {
            write_program_info(out.reborrow().get(i as u32), p);
        }
        Ok(())
    }

    async fn proposals(
        self: Rc<Self>,
        _: hub::ProposalsParams,
        mut results: hub::ProposalsResults,
    ) -> std::result::Result<(), capnp::Error> {
        let list = self.core.proposals().await.map_err(rpc_err)?;
        let mut out = results.get().init_proposals(list.len() as u32);
        for (i, p) in list.iter().enumerate() {
            write_proposal(out.reborrow().get(i as u32), p);
        }
        Ok(())
    }
}

struct SessionImpl {
    core: Arc<HubCore>,
    program: String,
    _live: LiveGuard,
}

impl session::Server for SessionImpl {
    async fn push(
        self: Rc<Self>,
        params: session::PushParams,
        mut results: session::PushResults,
    ) -> std::result::Result<(), capnp::Error> {
        let activities = read_activities(params.get()?.get_activities()?).map_err(rpc_err)?;
        let outcome = self.core.push(&self.program, activities).await.map_err(rpc_err)?;
        write_push_outcome(results.get(), &outcome);
        Ok(())
    }

    async fn pull(
        self: Rc<Self>,
        params: session::PullParams,
        mut results: session::PullResults,
    ) -> std::result::Result<(), capnp::Error> {
        let (since, limit) = {
            let p = params.get()?;
            (read_activity_id(p.get_since()?).map_err(rpc_err)?, p.get_limit())
        };
        let limit = if limit == 0 { 200 } else { limit as usize };
        let batch = self.core.pull(&self.program, since, limit).await.map_err(rpc_err)?;
        let mut r = results.get();
        write_activities(r.reborrow().init_activities(batch.activities.len() as u32), &batch.activities).map_err(rpc_err)?;
        write_activity_id(r.reborrow().init_last(), batch.last);
        r.set_more(batch.more);
        Ok(())
    }

    async fn ack(
        self: Rc<Self>,
        params: session::AckParams,
        _: session::AckResults,
    ) -> std::result::Result<(), capnp::Error> {
        if let Some(up_to) = read_activity_id(params.get()?.get_up_to()?).map_err(rpc_err)? {
            self.core.ack(&self.program, up_to).await.map_err(rpc_err)?;
        }
        Ok(())
    }

    async fn subscribe(
        self: Rc<Self>,
        params: session::SubscribeParams,
        mut results: session::SubscribeResults,
    ) -> std::result::Result<(), capnp::Error> {
        let listener = params.get()?.get_listener()?;
        let store = self.core.store();
        let mut rx = store.subscribe();
        let core = self.core.clone();
        let program = self.program.clone();
        let reads = core.manifest(&program).await.map_err(rpc_err)?.reads;
        let task = tokio::task::spawn_local(async move {
            loop {
                match rx.recv().await {
                    Ok(a) => {
                        if !core.visible_to(&program, &reads, &a).await {
                            continue;
                        }
                        if notify(&listener, &a).await.is_err() {
                            break; // listener gone
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue, // the program catches up with `pull`
                    Err(RecvError::Closed) => break,
                }
            }
        });
        results.get().set_subscription(new_client(SubscriptionImpl { task: RefCell::new(Some(task)) }));
        Ok(())
    }
}

/// Dropping the capability ends the subscription.
struct SubscriptionImpl {
    task: RefCell<Option<tokio::task::JoinHandle<()>>>,
}

impl subscription::Server for SubscriptionImpl {}

impl Drop for SubscriptionImpl {
    fn drop(&mut self) {
        if let Some(t) = self.task.borrow_mut().take() {
            t.abort();
        }
    }
}

async fn notify(listener: &change_listener::Client, a: &Activity) -> std::result::Result<(), capnp::Error> {
    let cs = a.change_set().map_err(rpc_err)?;
    let mut req = listener.change_request();
    {
        let change = req.get().init_change();
        match (a.kind, cs.after, cs.before) {
            (ActivityKind::Create, Some(o), _) => write_envelope(change.init_created(), &o.envelope),
            (_, Some(o), _) => write_envelope(change.init_updated(), &o.envelope),
            (_, None, _) => {
                let mut b = change;
                fs_hub_proto::write_uuid(b.reborrow().init_deleted(), Some(a.object.as_uuid()));
            }
        }
    }
    req.send().await
}

// ---- listening -----------------------------------------------------------------------------------

/// Serve the hub on an already bound listener until `shutdown` resolves. Must run on a `LocalSet`.
pub async fn serve(core: Arc<HubCore>, listener: UnixListener, socket: PathBuf, shutdown: impl Future<Output = ()>) {
    let owner = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&socket).expect("socket metadata"));
    let bootstrap: hub::Client = new_client(HubImpl { core });
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                // Peer check: same user as the socket owner. (Real peer authentication is future work.)
                match stream.peer_cred() {
                    Ok(c) if c.uid() == owner => {}
                    _ => { tracing::warn!("rejected a connection from another user"); continue; }
                }
                spawn_connection(stream, bootstrap.clone());
            }
        }
    }
}

fn spawn_connection(stream: UnixStream, bootstrap: hub::Client) {
    let (r, w) = stream.into_split();
    let network = twoparty::VatNetwork::new(r.compat(), w.compat_write(), Side::Server, Default::default());
    let rpc = RpcSystem::new(Box::new(network), Some(bootstrap.client));
    tokio::task::spawn_local(async move {
        if let Err(e) = rpc.await {
            tracing::debug!("connection ended: {e}");
        }
    });
}

/// Bind the socket: refuse if a live hub already owns it, remove a stale file, tighten permissions.
pub async fn bind_socket(path: &Path) -> Result<UnixListener> {
    if let Some(dir) = path.parent() {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
        }
    }
    if path.exists() {
        if UnixStream::connect(path).await.is_ok() {
            return Err(Error::AlreadyRunning(path.display().to_string()));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}
