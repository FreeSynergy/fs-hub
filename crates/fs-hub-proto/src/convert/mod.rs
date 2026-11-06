//! Conversions between wire (Cap'n Proto) and `fs-store` / plain Rust types.
//!
//! Pattern: Adapter — one file per wire struct; callers (server, client) never touch the
//! generated builders/readers directly.

mod activity;
mod envelope;
mod ids;
mod lists;
mod manifest;
mod outcome;
mod program;
mod proposal;

pub use activity::{read_activities, read_activity, write_activities, write_activity};
pub use envelope::{read_envelope, write_envelope};
pub use ids::{read_activity_id, read_uuid, write_activity_id, write_uuid};
pub use manifest::{read_manifest, write_manifest};
pub use outcome::{read_pull_batch, read_push_outcome, write_push_outcome};
pub use program::{read_program_info, write_program_info};
pub use proposal::{read_proposal, write_proposal};
