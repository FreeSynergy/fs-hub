//! Spawning the RPC thread.
//!
//! Pattern: Factory — builds the thread, waits for the registration result, returns the session.

use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

use super::run::run;
use crate::{Error, HubSession, Manifest, Result};

/// Connect and register on a private thread; returns the session handle.
pub(crate) async fn start(socket: PathBuf, manifest: Manifest) -> Result<HubSession> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (ready_tx, ready_rx) = oneshot::channel::<Result<()>>();
    std::thread::Builder::new()
        .name("fs-hub-client".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
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
