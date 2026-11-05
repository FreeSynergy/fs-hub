/// Errors of the hub daemon.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
    #[error("contract: {0}")]
    Proto(#[from] fs_hub_proto::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("a hub is already running on {0}")]
    AlreadyRunning(String),
}

pub type Result<T> = std::result::Result<T, Error>;
