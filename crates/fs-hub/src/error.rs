//! Errors of the hub daemon.
//!
//! Pattern: Adapter (error mapping) — foreign errors funnel into one enum via `?`.

/// Errors of the hub daemon.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Object store error.
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
    /// Contract conversion error.
    #[error("contract: {0}")]
    Proto(#[from] fs_hub_proto::Error),
    /// I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// JSON error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Bad request (unknown program, bad manifest, ...).
    #[error("{0}")]
    Invalid(String),
    /// Another hub owns the socket.
    #[error("a hub is already running on {0}")]
    AlreadyRunning(String),
}

/// Result alias for the daemon.
pub type Result<T> = std::result::Result<T, Error>;
