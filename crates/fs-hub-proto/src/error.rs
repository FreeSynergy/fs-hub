//! Errors of the contract conversions.
//!
//! Pattern: Adapter (error mapping) — foreign errors (capnp, store, json) funnel into one enum via `?`.

/// Errors of the contract conversions.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Cap'n Proto read/write error.
    #[error("cap'n proto: {0}")]
    Capnp(#[from] capnp::Error),
    /// Unknown enum/union variant.
    #[error("not in schema: {0}")]
    Schema(#[from] capnp::NotInSchema),
    /// Text was not valid UTF-8.
    #[error("utf-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// Store error (ids, kinds).
    #[error("store: {0}")]
    Store(#[from] fs_objects::Error),
    /// JSON payload error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Structurally valid but semantically wrong data.
    #[error("invalid: {0}")]
    Invalid(String),
}

/// Result alias for the contract crate.
pub type Result<T> = std::result::Result<T, Error>;
