//! A hub on its own thread.
//!
//! Pattern: RAII / Guard — `Daemon` starts the hub, and stops and joins it on `stop` or drop.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::oneshot;

use super::config::Config;
use super::run::run;
use crate::{Error, HubCore, Result};

/// A hub running on its own thread (tests, examples, embedding).
pub struct Daemon {
    core: Arc<HubCore>,
    socket: PathBuf,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<Result<()>>>,
}

impl Daemon {
    /// Start the hub and return once it listens.
    pub fn spawn(config: Config) -> Result<Self> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let socket = config.socket.clone();
        let thread = std::thread::Builder::new()
            .name("fs-hub".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let local = tokio::task::LocalSet::new();
                local.block_on(
                    &rt,
                    run(config, Some(ready_tx), async {
                        let _ = stop_rx.await;
                    }),
                )
            })?;
        match ready_rx.recv() {
            Ok(core) => Ok(Self {
                core,
                socket,
                stop: Some(stop_tx),
                thread: Some(thread),
            }),
            Err(_) => match thread.join() {
                Ok(Err(e)) => Err(e),
                _ => Err(Error::Invalid("hub thread ended before it listened".into())),
            },
        }
    }

    /// The socket the hub listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The hub's core (e.g. to read its store).
    pub fn core(&self) -> &Arc<HubCore> {
        &self.core
    }

    /// Stop and wait.
    pub fn stop(mut self) -> Result<()> {
        self.shutdown()
    }

    fn shutdown(&mut self) -> Result<()> {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        match self.thread.take() {
            Some(t) => t
                .join()
                .unwrap_or_else(|_| Err(Error::Invalid("hub thread panicked".into()))),
            None => Ok(()),
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
