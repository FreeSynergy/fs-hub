//! No echo: pulled changes must not travel back.
//!
//! Pattern: Specification-driven cleanup — decides which prefix of the log is ours and marks it synced.

use std::collections::BTreeSet;

use fs_store::{ActivityFilter, ActivityId, Store};

use crate::Result;

/// What came from the hub must not travel back as new work: mark the log entries our own `apply`
/// wrote as synced. `mark_synced` acknowledges a prefix of the log, so entries after the first
/// foreign unsynced one stay (the hub drops them as duplicates of what it already holds).
pub(super) async fn mark_applied_synced(
    store: &dyn Store,
    ours: &BTreeSet<ActivityId>,
) -> Result<()> {
    let Some(&max_ours) = ours.last() else {
        return Ok(());
    };
    let unsynced = store
        .activities(&ActivityFilter {
            unsynced_only: true,
            ..Default::default()
        })
        .await?;
    let first_foreign = unsynced.iter().map(|a| a.id).find(|id| !ours.contains(id));
    let upto = match first_foreign {
        None => Some(max_ours),
        Some(f) => ours.range(..f).next_back().copied(),
    };
    if let Some(upto) = upto {
        store.mark_synced(upto).await?;
    }
    Ok(())
}
