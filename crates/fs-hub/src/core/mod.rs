//! The hub's connecting logic, independent of the RPC layer.
//!
//! Pattern: Facade over Strategies — [`HubCore`] composes the traits below; see `DESIGN.md`.

mod activity;
mod apply;
mod collector;
mod cursors;
mod hub;
mod log_source;
mod originals;
mod presence;
mod proposals;
mod registry;
mod state;
mod visibility;

pub use activity::{ActivitySink, ActivitySource};
pub use collector::ObjectCollector;
pub use cursors::{CursorRepository, StoreCursors};
pub use hub::HubCore;
pub use log_source::LogSource;
pub use originals::{OriginalTimes, StoreOriginals};
pub use presence::{LiveGuard, Presence};
pub use proposals::{ProposalEngine, SharedIdentity};
pub use registry::{ProgramLock, ProgramRegistry, StoredRegistry};
pub use state::ProgramState;
pub use visibility::Visibility;
