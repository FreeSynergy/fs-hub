//! Errors of the hub daemon.
//!
//! Pattern: Adapter (error mapping) — foreign errors funnel into one enum via `?`.

/// Errors of the hub daemon.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Object store error.
    #[error("store: {0}")]
    Store(#[from] fs_objects::Error),
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
    /// The list of marked objects is gone: what it kept back could go out.
    #[error(
        "{0} is gone, and old content of marked objects could be delivered; restore it (see DESIGN.md)"
    )]
    MarksLost(String),
    /// Another hub owns the socket.
    #[error("a hub is already running on {0}")]
    AlreadyRunning(String),
}

impl Error {
    /// A failure that may pass: nothing is wrong with what was asked, so no cursor may move.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Store(e) => fs_hub_proto::is_transient(e),
            Self::Io(_) => true,
            _ => false,
        }
    }
}

/// Result alias for the daemon.
pub type Result<T> = std::result::Result<T, Error>;
