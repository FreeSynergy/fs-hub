//! Contract constants and the default socket location.
//!
//! Pattern: Registry of well-known values — the single place where version and path convention live.

/// Version of this contract; programs send the version they were built for in their manifest.
pub const CONTRACT_VERSION: &str = "0.1";

/// Default socket path: `$XDG_RUNTIME_DIR/freesynergy/hub.sock` (ADR 0019); `/tmp/freesynergy-<uid>`-less
/// fallback is deliberately not offered: without a runtime dir the caller must pass a path.
pub fn default_socket_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(
        std::path::PathBuf::from(dir)
            .join("freesynergy")
            .join("hub.sock"),
    )
}
