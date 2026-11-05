/// Errors of the client library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nobody listens on the socket (used internally to turn into `HubStatus::Absent`).
    #[error("no hub is listening")]
    NoHub,
    #[error("hub connection closed")]
    Closed,
    #[error("rpc: {0}")]
    Rpc(#[from] capnp::Error),
    #[error("contract: {0}")]
    Proto(#[from] fs_hub_proto::Error),
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol: {0}")]
    Protocol(String),
}

pub type Result<T> = std::result::Result<T, Error>;
