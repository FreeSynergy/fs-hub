//! Purge an object's history only after the hub has it.
//!
//! Pattern: Command (a checked sequence behind one call) — `purge_object` deletes the unsynced log entries
//! too. Marking an object sensitive and purging before the next sync would leave the hub with the
//! unmarked copy and nothing to push the mark with (ADR 0029). So: sync first, check that nothing of the
//! object is left unsynced, then purge. Anything that goes wrong before the purge means no purge.

use fs_store::{ActivityFilter, ObjectId, PurgeReport, Store};

use super::once::sync_once;
use crate::{Error, HubSession, Result};

pub(crate) async fn sync_then_purge(
    hub: &HubSession,
    store: &dyn Store,
    object: ObjectId,
) -> Result<PurgeReport> {
    sync_once(hub, store).await?;
    let open = store
        .activities(&ActivityFilter {
            object: Some(object),
            unsynced_only: true,
            limit: Some(1),
            ..Default::default()
        })
        .await?;
    if !open.is_empty() {
        // Held back by the hub (another program's mark), or written while syncing: not safe to purge.
        return Err(Error::NotSynced(object));
    }
    Ok(store.purge_object(object).await?)
}
