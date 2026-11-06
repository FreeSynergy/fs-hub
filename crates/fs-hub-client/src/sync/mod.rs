//! `sync_once`: the program's side of ADR 0019 point 4 (activity log in both directions).
//!
//! Pattern: Template method — fixed skeleton (push all unsynced, pull all, push what is left), with
//! the steps in their own files.

mod apply;
mod once;
mod pull;
mod push;
mod report;

pub(crate) use once::sync_once;
pub use report::SyncReport;
