//! Hub configuration.
//!
//! Pattern: Value object with a Factory for the standard locations (ADR 0019).

use std::path::PathBuf;

/// Where the hub keeps its socket and data.
#[derive(Clone, Debug)]
pub struct Config {
    /// Unix socket, default `$XDG_RUNTIME_DIR/freesynergy/hub.sock`.
    pub socket: PathBuf,
    /// Directory of `objects.db` and `state.db`, default `$XDG_DATA_HOME/freesynergy/hub`.
    pub data_dir: PathBuf,
}

impl Config {
    /// Explicit locations.
    pub fn new(socket: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            data_dir: data_dir.into(),
        }
    }

    /// The standard locations (ADR 0019); `None` if the environment gives no runtime/home directory.
    pub fn default_locations() -> Option<Self> {
        let socket = fs_hub_proto::default_socket_path()?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(Self {
            socket,
            data_dir: data.join("freesynergy").join("hub"),
        })
    }
}
