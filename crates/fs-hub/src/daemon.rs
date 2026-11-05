//! Running the hub: configuration, a background-thread handle for tests/examples, and the
//! blocking entry point of the binary.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::core::HubCore;
use crate::{Error, Result, server};

/// Where the hub keeps its socket and data.
#[derive(Clone, Debug)]
pub struct Config {
    /// Unix socket, default `$XDG_RUNTIME_DIR/freesynergy/hub.sock`.
    pub socket: PathBuf,
    /// Directory of `objects.db` and `state.db`, default `$XDG_DATA_HOME/freesynergy/hub`.
    pub data_dir: PathBuf,
}

impl Config {
    pub fn new(socket: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self { socket: socket.into(), data_dir: data_dir.into() }
    }

    /// The standard locations (ADR 0019); `None` if the environment gives no runtime/home directory.
    pub fn default_locations() -> Option<Self> {
        let socket = fs_hub_proto::default_socket_path()?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(Self { socket, data_dir: data.join("freesynergy").join("hub") })
    }
}

async fn run(config: Config, ready: Option<std::sync::mpsc::Sender<Arc<HubCore>>>, shutdown: impl Future<Output = ()>) -> Result<()> {
    let core = HubCore::open(&config.data_dir).await?;
    let listener = server::bind_socket(&config.socket).await?;
    if let Some(tx) = ready {
        let _ = tx.send(core.clone());
    }
    tracing::info!("hub listening on {}", config.socket.display());
    server::serve(core, listener, config.socket.clone(), shutdown).await;
    let _ = std::fs::remove_file(&config.socket);
    Ok(())
}

/// Run the hub on the current thread until Ctrl-C / SIGTERM (the binary's `main`).
pub fn run_blocking(config: Config) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async {
        let stop = async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        };
        run(config, None, stop).await
    })
}

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
        let thread = std::thread::Builder::new().name("fs-hub".into()).spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, run(config, Some(ready_tx), async {
                let _ = stop_rx.await;
            }))
        })?;
        match ready_rx.recv() {
            Ok(core) => Ok(Self { core, socket, stop: Some(stop_tx), thread: Some(thread) }),
            Err(_) => match thread.join() {
                Ok(Err(e)) => Err(e),
                _ => Err(Error::Invalid("hub thread ended before it listened".into())),
            },
        }
    }

    pub fn socket(&self) -> &std::path::Path {
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
            Some(t) => t.join().unwrap_or_else(|_| Err(Error::Invalid("hub thread panicked".into()))),
            None => Ok(()),
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
