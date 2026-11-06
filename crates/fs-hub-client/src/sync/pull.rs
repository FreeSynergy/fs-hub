//! Pull step: hub -> local store.
//!
//! Pattern: Iterator-style batching — pulls until nothing more is waiting, applies each batch,
//! marks our own writes synced and acknowledges to the hub.

use std::collections::BTreeSet;

use fs_store::{ActivityId, Store};

use super::apply::apply;
use super::mark::mark_applied_synced;
use super::report::SyncReport;
use crate::{HubSession, Result};

const PULL_BATCH: u32 = 200;

pub(super) async fn pull_all(
    hub: &HubSession,
    store: &dyn Store,
    report: &mut SyncReport,
) -> Result<()> {
    loop {
        let batch = hub.pull(None, PULL_BATCH).await?;
        report.pulled += batch.activities.len();
        let mut ours: BTreeSet<ActivityId> = BTreeSet::new();
        for a in &batch.activities {
            match apply(store, a).await {
                Ok(created) => {
                    if !created.is_empty() {
                        report.applied += 1;
                    }
                    ours.extend(created);
                }
                Err(e) => report
                    .failed
                    .push(format!("{} {}: {e}", a.kind.as_str(), a.id)),
            }
        }
        mark_applied_synced(store, &ours).await?;
        if let Some(last) = batch.last {
            hub.ack(last).await?;
        }
        if !batch.more || batch.activities.is_empty() {
            return Ok(());
        }
    }
}
