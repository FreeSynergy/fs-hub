//! `fs-hub`: the FreeSynergy hub, one per device (ADR 0019). It collects the objects of all programs in
//! its own `fs-objects`, keeps a cursor per program, and proposes matches. Programs never find each
//! other, only the hub. No domain logic lives here.
//!
//! Pattern: Facade over Strategies, with an Adapter to Cap'n Proto — see `DESIGN.md` for the map.
//!
//! * [`HubCore`]: the connecting logic (testable without RPC), built from the traits
//!   [`ProgramRegistry`], [`ActivitySink`], [`ActivitySource`], [`ProposalEngine`], [`CursorRepository`].
//! * [`server`]: Cap'n Proto RPC over a Unix socket (`schema/hub.capnp`).
//! * [`Daemon`]: start/stop the whole thing (own thread), used by the binary's tests and examples.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// Noise for this crate: error/panic docs on every fn, `#[must_use]` on getters, `unwrap` on
// poisoned std mutexes is intended, and capnp list sizes/indices are small `as` casts.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::doc_markdown
)]

mod core;
mod daemon;
mod error;
pub mod server;

pub use core::HeldWrites;
pub use core::{
    ActivitySink, ActivitySource, CursorRepository, HubCore, LiveGuard, LogSource, MarkedRemovals,
    ObjectCollector, OriginalTimes, Presence, ProgramLock, ProgramRegistry, ProgramState,
    ProposalEngine, Reach, Removal, SharedIdentity, StoreCursors, StoreMarked, StoreOriginals,
    StoredRegistry, Visibility,
};
pub use daemon::{Config, Daemon, run_blocking};
pub use error::{Error, Result};
