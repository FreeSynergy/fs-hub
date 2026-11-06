//! `fs-hub-client`: the library a program embeds to talk to the FreeSynergy hub (ADR 0019).
//!
//! Pattern: Facade — [`connect`] / [`connect_at`] hide socket, RPC thread and registration behind
//! a [`HubStatus`] (State: `Connected` or `Absent`) and a [`HubSession`].
//!
//! * [`connect`] looks for the hub's socket. No hub: [`HubStatus::Absent`] and the program simply keeps
//!   working alone, writing its activity log; [`offer_install`] gives the text to ask the user.
//! * With a hub: register with a [`Manifest`], then [`HubSession::sync_once`] pushes the unsynced part of
//!   the log, pulls what other programs wrote (only declared types) into the program's store, and marks
//!   things synced.
//!
//! The RPC runs on a private thread (capnp-rpc is not `Send`), so the session is `Send + Sync`
//! and works from any tokio runtime.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// Noise for this crate: error docs on every fn, `#[must_use]` on getters, and small `as` casts for
// capnp list sizes.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::doc_markdown
)]

mod command;
mod connect;
mod error;
mod install;
mod session;
mod status;
mod sync;
mod worker;

pub use connect::{connect, connect_at};
pub use error::{Error, Result};
pub use fs_hub_proto::{ChangeNotice, Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome};
pub use install::offer_install;
pub use session::HubSession;
pub use status::HubStatus;
pub use sync::SyncReport;
