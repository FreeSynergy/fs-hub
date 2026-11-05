//! `fs-hub`: the FreeSynergy hub, one per device (ADR 0019). It collects the objects of all programs in
//! its own `fs-store`, keeps a cursor per program, and proposes matches. Programs never find each
//! other, only the hub. No domain logic lives here.
//!
//! * [`HubCore`]: the connecting logic (testable without RPC).
//! * [`server`]: Cap'n Proto RPC over a Unix socket (`schema/hub.capnp`).
//! * [`Daemon`]: start/stop the whole thing (own thread), used by the binary's tests and examples.

mod core;
mod daemon;
mod error;
pub mod server;

pub use core::HubCore;
pub use daemon::{Config, Daemon, run_blocking};
pub use error::{Error, Result};
