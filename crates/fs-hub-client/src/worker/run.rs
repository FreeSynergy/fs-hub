//! The worker's main loop.
//!
//! Pattern: Command executor — connects, registers, then answers commands until the session
//! handle is dropped; closing the connection ends the hub-side session.

use std::cell::RefCell;
use std::path::PathBuf;

use fs_hub_proto::common_capnp::subscription;
use fs_hub_proto::hub_capnp::hub;
use fs_hub_proto::reader_options;
use fs_interfaces::rpc::{Connection, connect_with};
use tokio::sync::{mpsc, oneshot};

use super::calls::{ack, programs, proposals, pull, push, register};
use super::listener::subscribe;
use crate::command::Cmd;
use crate::{Error, Manifest, Result};

pub(super) async fn run(
    socket: PathBuf,
    manifest: Manifest,
    mut cmds: mpsc::UnboundedReceiver<Cmd>,
    ready: oneshot::Sender<Result<()>>,
) {
    // Refuses a socket served by another user. Dropping the connection closes it.
    let connection: Connection<hub::Client> = match connect_with(&socket, reader_options()).await {
        Ok(connection) => connection,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            let _ = ready.send(Err(Error::NoHub));
            return;
        }
        Err(e) => {
            let _ = ready.send(Err(e.into()));
            return;
        }
    };
    let hub = &connection.client;

    let session = match register(hub, &manifest).await {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let subscriptions: RefCell<Vec<subscription::Client>> = RefCell::new(vec![]);

    while let Some(cmd) = cmds.recv().await {
        match cmd {
            Cmd::Push(acts, reply) => {
                let _ = reply.send(push(&session, &acts).await);
            }
            Cmd::Pull(since, limit, envelope_only, reply) => {
                let _ = reply.send(pull(&session, since, limit, envelope_only).await);
            }
            Cmd::Ack(id, reply) => {
                let _ = reply.send(ack(&session, id).await);
            }
            Cmd::Programs(reply) => {
                let _ = reply.send(programs(hub).await);
            }
            Cmd::Proposals(reply) => {
                let _ = reply.send(proposals(hub).await);
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
    drop(connection);
}
