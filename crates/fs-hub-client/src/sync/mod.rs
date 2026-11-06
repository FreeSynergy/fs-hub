//! `sync_once`: the program's side of ADR 0019 point 4 (activity log in both directions).
//!
//! Pattern: Template method — fixed skeleton (push all unsynced, then pull all), with the steps in
//! their own files.

mod apply;
mod mark;
mod pull;
mod push;
mod report;

pub use report::SyncReport;

use fs_store::Store;

use crate::{HubSession, Result};

pub(crate) async fn sync_once(hub: &HubSession, store: &dyn Store) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    push::push_all(hub, store, &mut report).await?;
    pull::pull_all(hub, store, &mut report).await?;
    Ok(report)
}
