//! Binding the hub's Unix socket.
//!
//! Pattern: Factory — creates the listener from a path: refuses a second live hub, removes a
//! stale file, creates the directory 0700 and the socket 0600.

use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

use crate::{Error, Result};

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
