/// Errors of the contract conversions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cap'n proto: {0}")]
    Capnp(#[from] capnp::Error),
    #[error("not in schema: {0}")]
    Schema(#[from] capnp::NotInSchema),
    #[error("utf-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
