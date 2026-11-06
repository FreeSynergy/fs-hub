//! Errors of the client library.
//!
//! Pattern: Adapter (error mapping) — foreign errors funnel into one enum via `?`.

/// Errors of the client library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nobody listens on the socket (used internally to turn into `HubStatus::Absent`).
    #[error("no hub is listening")]
    NoHub,
    /// The connection (RPC thread) is gone.
    #[error("hub connection closed")]
    Closed,
    /// RPC failure.
    #[error("rpc: {0}")]
    Rpc(#[from] capnp::Error),
    /// Contract conversion failure.
    #[error("contract: {0}")]
    Proto(#[from] fs_hub_proto::Error),
    /// Local store failure.
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
    /// I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The hub answered something the protocol does not allow.
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Result alias for the client.
pub type Result<T> = std::result::Result<T, Error>;
