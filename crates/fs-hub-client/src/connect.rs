//! Finding and registering with the hub.
//!
//! Pattern: Factory — builds a [`HubStatus`] from a socket path: no socket or a stale one gives
//! `Absent`, a live hub gives a registered session.

use std::path::{Path, PathBuf};

use crate::worker;
use crate::{Error, HubStatus, Manifest, Result};

/// Look for the hub at the default socket (`$XDG_RUNTIME_DIR/freesynergy/hub.sock`) and register.
pub async fn connect(manifest: Manifest) -> Result<HubStatus> {
    match fs_hub_proto::default_socket_path() {
        Some(path) => connect_at(&path, manifest).await,
        None => Ok(HubStatus::Absent),
    }
}

/// Like [`connect`] with an explicit socket path.
pub async fn connect_at(socket: &Path, manifest: Manifest) -> Result<HubStatus> {
    if !socket.exists() {
        return Ok(HubStatus::Absent);
    }
    match worker::start(PathBuf::from(socket), manifest).await {
        Ok(session) => Ok(HubStatus::Connected(session)),
        Err(Error::NoHub) => Ok(HubStatus::Absent), // stale socket file
        Err(e) => Err(e),
    }
}
