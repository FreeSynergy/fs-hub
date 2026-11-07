//! `sync_once`: the program's side of ADR 0019 point 4 (activity log in both directions).
//!
//! Pattern: Template method — fixed skeleton (push all unsynced, pull all, push what is left), with
//! the steps in their own files.

mod apply;
#[cfg(test)]
mod apply_redacted;
mod once;
mod pull;
mod purge;
mod push;
mod redacted;
#[cfg(test)]
mod redacted_tests;
mod report;

pub(crate) use once::sync_once;
pub(crate) use purge::sync_then_purge;
pub use report::SyncReport;
