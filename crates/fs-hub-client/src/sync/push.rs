//! Push step: unsynced local log -> hub.
//!
//! Pattern: Iterator-style batching — takes the log in batches, marks each synced once the hub
//! acknowledged it.

use fs_store::{ActivityFilter, Store};

use super::report::SyncReport;
use crate::{Error, HubSession, Result};

const PUSH_BATCH: usize = 100;

pub(super) async fn push_all(
    hub: &HubSession,
    store: &dyn Store,
    report: &mut SyncReport,
) -> Result<()> {
    loop {
        let batch = store
            .activities(&ActivityFilter {
                unsynced_only: true,
                limit: Some(PUSH_BATCH),
                ..Default::default()
            })
            .await?;
        let Some(batch_last) = batch.last().map(|a| a.id) else {
            return Ok(());
        };
        report.pushed += batch.len();
        let outcome = hub.push(batch).await?;
        report.accepted += outcome.accepted;
        report.skipped += outcome.skipped;
        report.withheld += outcome.withheld;
        report.problems.extend(outcome.problems);
        let acked = outcome
            .last_accepted
            .ok_or_else(|| Error::Protocol("hub acknowledged nothing".into()))?;
        if acked < batch_last {
            // Would loop forever; the hub's cursor is behind what it just processed.
            return Err(Error::Protocol(format!(
                "hub acknowledged {acked}, expected at least {batch_last}"
            )));
        }
        store.mark_synced(batch_last).await?;
    }
}
