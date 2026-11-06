//! Running the hub: configuration, the blocking entry point, and a background-thread handle.
//!
//! Pattern: Facade — three small entry points ([`Config`], [`run_blocking`], [`Daemon`]) over
//! core + server.

mod config;
mod handle;
mod run;

pub use config::Config;
pub use handle::Daemon;
pub use run::run_blocking;
