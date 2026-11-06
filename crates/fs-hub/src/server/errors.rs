//! Error mapping from the core to RPC.
//!
//! Pattern: Adapter (error mapping) — any displayable core error becomes a `capnp::Error::failed`.

pub(super) fn rpc_err(e: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(e.to_string())
}
