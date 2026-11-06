//! The hub's main loop.
//!
//! Pattern: Template method — open core, bind socket, (announce readiness), serve until the
//! shutdown future resolves, clean up the socket; the callers differ only in the shutdown trigger.

use std::sync::Arc;

use super::config::Config;
use crate::{HubCore, Result, server};

/// Open, bind, announce `core` on `ready`, serve until `shutdown`, remove the socket.
pub(super) async fn run(
    config: Config,
    ready: Option<std::sync::mpsc::Sender<Arc<HubCore>>>,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    let core = HubCore::open(&config.data_dir).await?;
    let listener = server::bind_socket(&config.socket).await?;
    if let Some(tx) = ready {
        let _ = tx.send(core.clone());
    }
    tracing::info!("hub listening on {}", config.socket.display());
    server::serve(core.clone(), listener, config.socket.clone(), shutdown).await;
    core.flush().await?;
    let _ = std::fs::remove_file(&config.socket);
    Ok(())
}

/// Run the hub on the current thread until Ctrl-C / SIGTERM (the binary's `main`).
pub fn run_blocking(config: Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        let stop = async {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        };
        run(config, None, stop).await
    })
}
