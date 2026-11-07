//! The Cap'n Proto RPC layer (`schema/hub.capnp`) over the [`HubCore`](crate::HubCore).
//!
//! Pattern: Adapter — capnp-rpc servers (`Hub`, `Session`, `Subscription`) translate calls into
//! core calls and results back; runs on a `LocalSet` (capnp-rpc is not `Send`).

mod errors;
mod hub_service;
mod listen;
mod session_service;
mod socket;
mod subscription;
#[cfg(test)]
mod subscription_tests;

pub use listen::serve;
pub use socket::bind_socket;
