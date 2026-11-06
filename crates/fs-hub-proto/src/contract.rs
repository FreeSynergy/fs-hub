//! Contract constants and the default socket location.
//!
//! Pattern: Registry of well-known values — the single place where version and path convention live.

/// Version of this contract; programs send the version they were built for in their manifest.
pub const CONTRACT_VERSION: &str = "0.1";

/// Default socket path: `$XDG_RUNTIME_DIR/freesynergy/hub.sock` (ADR 0019), the same place as every
/// program's. Without a runtime directory there is no fallback: the caller must pass a path.
pub fn default_socket_path() -> Option<std::path::PathBuf> {
    fs_interfaces::rpc::default_socket("hub")
}
