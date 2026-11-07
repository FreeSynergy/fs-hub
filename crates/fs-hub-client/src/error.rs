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
    Store(#[from] fs_objects::Error),
    /// I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// A sync stopped on `cause`; `report` says what it did until then, also what was already passed.
    #[error("sync stopped: {cause}")]
    Stopped {
        /// Why it stopped.
        cause: Box<Error>,
        /// What it did until then.
        report: Box<crate::SyncReport>,
    },
    /// The object has log entries the hub has not taken; purging them would lose them.
    #[error("object {0:?} has unsynced log entries: not purged")]
    NotSynced(fs_objects::ObjectId),
    /// The hub answered something the protocol does not allow.
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Result alias for the client.
pub type Result<T> = std::result::Result<T, Error>;
