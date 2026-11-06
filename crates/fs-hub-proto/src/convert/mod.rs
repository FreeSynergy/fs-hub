//! Conversions between wire (Cap'n Proto) and `fs-store` / plain Rust types.
//!
//! Pattern: Adapter — one file per wire struct; callers (server, client) never touch the
//! generated builders/readers directly.

mod activity;
mod ids;
mod images;
mod lists;
mod manifest;
mod outcome;
mod program;
mod proposal;

pub use activity::{read_pushed_list, write_pulled_list, write_pushed_list};
pub use fs_interfaces::rpc::{read_envelope, write_envelope};
pub use ids::{read_activity_id, read_uuid, write_activity_id, write_uuid};
pub use images::{images_of, summary_of};
pub use manifest::{read_manifest, write_manifest};
pub use outcome::{read_pull_batch, read_push_outcome, write_push_outcome};
pub use program::{read_program_info, write_program_info};
pub use proposal::{read_proposal, write_proposal};
