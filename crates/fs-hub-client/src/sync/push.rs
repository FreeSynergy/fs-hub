//! Push step: unsynced local log -> hub.
//!
//! Pattern: Iterator-style batching — takes the log in small batches, marks each synced once the hub
//! acknowledged it; `HubSession::push` cuts a batch to what fits a message. Two kinds of entries do
//! not go, and are marked all the same:
//! * what came from the hub (`hub:*`). This is what stops an echo: after a crash, or behind a foreign
//!   entry, such an entry may still be unsynced, and pushing it would roll the hub back to an old
//!   version.
//! * a log-only entry about a sensitive object, or one the store no longer has, so nobody can say it
//!   was not sensitive: it counts as withheld.

use fs_hub_proto::ScanSize;
use fs_store::{Activity, ActivityFilter, Store};

use super::apply::HUB_ACTOR_PREFIX;
use super::report::SyncReport;
use crate::{Error, HubSession, Result};

async fn may_tell(store: &dyn Store, a: &Activity) -> Result<bool> {
    if a.kind.changes_state() {
        return Ok(true); // the images say for themselves, `push_plan` looks at them
    }
    let object = store.get(a.object).await?;
    Ok(object.is_some_and(|o| o.envelope.sensitive.is_empty()))
}

pub(super) async fn push_all(
    hub: &HubSession,
    store: &dyn Store,
    report: &mut SyncReport,
) -> Result<()> {
    let mut scan = ScanSize::default();
    loop {
        let batch = store
            .activities(&ActivityFilter {
                unsynced_only: true,
                limit: Some(scan.next()),
                ..Default::default()
            })
            .await?;
        let Some(batch_last) = batch.last().map(|a| a.id) else {
            return Ok(());
        };
        let mut send = Vec::with_capacity(batch.len());
        for a in batch {
            scan.saw(&a.payload);
            if a.actor.starts_with(HUB_ACTOR_PREFIX) {
                continue;
            }
            if may_tell(store, &a).await? {
                send.push(a);
            } else {
                report.withheld += 1;
            }
        }
        if let Some(send_last) = send.last().map(|a| a.id) {
            report.pushed += send.len();
            let outcome = hub.push(send).await?;
            report.accepted += outcome.accepted;
            report.skipped += outcome.skipped;
            report.withheld += outcome.withheld;
            report.problems.extend(outcome.problems);
            let acked = outcome
                .last_accepted
                .ok_or_else(|| Error::Protocol("hub acknowledged nothing".into()))?;
            if acked < send_last {
                // Would loop forever; the hub's cursor is behind what it just processed.
                return Err(Error::Protocol(format!(
                    "hub acknowledged {acked}, expected at least {send_last}"
                )));
            }
        }
        store.mark_synced(batch_last).await?;
    }
}
