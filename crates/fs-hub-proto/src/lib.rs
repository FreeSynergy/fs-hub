//! `fs-hub-proto`: the Cap'n Proto contract of the hub (`schema/hub.capnp`) and conversions between the
//! wire types and `fs-store` types. Shared by the daemon and the client library.
//!
//! The generated code (`common_capnp.rs`, `hub_capnp.rs`) is committed (ADR 0040); see `build.rs`.

#[allow(clippy::all, clippy::pedantic, unused_parens, dead_code)]
pub mod common_capnp;
#[allow(clippy::all, clippy::pedantic, unused_parens, dead_code)]
pub mod hub_capnp;

mod convert;
mod error;
mod types;

pub use convert::*;
pub use error::{Error, Result};
pub use types::*;

/// Version of this contract; programs send the version they were built for in their manifest.
pub const CONTRACT_VERSION: &str = "0.1";

/// Default socket path: `$XDG_RUNTIME_DIR/freesynergy/hub.sock` (ADR 0019); `/tmp/freesynergy-<uid>`-less
/// fallback is deliberately not offered: without a runtime dir the caller must pass a path.
pub fn default_socket_path() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(std::path::PathBuf::from(dir).join("freesynergy").join("hub.sock"))
}
