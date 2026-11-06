//! Push step: unsynced local log -> hub.
//!
//! Pattern: Iterator-style batching — takes the log in small batches, marks each synced once the hub
//! acknowledged it; `HubSession::push` cuts a batch to what fits a message. What goes depends on the
//! object as it is now, not as it was when the entry was written:
//! * what came from the hub (`hub:*`) never goes, and is marked all the same. This is what stops an
//!   echo: after a crash, or behind a foreign entry, such an entry may still be unsynced, and pushing it
//!   would roll the hub back to an old version.
//! * of an object marked sensitive now, a change goes as stubs only, so the history from before the mark
//!   stays home too. A log-only entry does not go, and neither does one about an object the store no
//!   longer has, as nobody can say it was not sensitive; both count as withheld.

use fs_hub_proto::ScanSize;
use fs_store::{Activity, ActivityFilter, Store};

use super::apply::HUB_ACTOR_PREFIX;
use super::report::SyncReport;
use crate::push_plan::stub_images;
use crate::{Error, HubSession, Result};

/// What may go of `a`, looking at the object as it is now.
async fn what_may_go(store: &dyn Store, a: Activity) -> Result<Option<Activity>> {
    let current = store.get(a.object).await?;
    let marked = current
        .as_ref()
        .is_some_and(|o| !o.envelope.sensitive.is_empty());
    if a.kind.changes_state() {
        return Ok(Some(if marked { stub_images(a) } else { a }));
    }
    Ok((current.is_some() && !marked).then_some(a))
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
            match what_may_go(store, a).await? {
                Some(a) => send.push(a),
                None => report.withheld += 1,
            }
        }
        if let Some(send_last) = send.last().map(|a| a.id) {
            let handed_over = send.len();
            let outcome = hub.push(send).await?;
            report.pushed += handed_over - outcome.refused as usize;
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
