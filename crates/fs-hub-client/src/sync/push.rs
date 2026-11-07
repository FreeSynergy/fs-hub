//! Push step: unsynced local log -> hub.
//!
//! Pattern: Iterator-style batching — takes the log in small batches, marks each synced once the hub
//! acknowledged it; `HubSession::push` cuts a batch to what fits a message. What goes depends on the
//! object as it is now, not as it was when the entry was written:
//! * what came from the hub (`hub:*`) never goes, and is marked all the same. This is what stops an
//!   echo: after a crash, or behind a foreign entry, such an entry may still be unsynced, and pushing it
//!   would roll the hub back to an old version.
//! * of an object marked sensitive now, a change goes as stubs only, so the history from before the mark
//!   stays home too. So does one marked and then deleted: its later entries say so. A log-only entry does
//!   not go, and neither does one about an object the store no longer has, as nobody can say it was not
//!   sensitive; both count as withheld.
//!
//! * an entry without state (purged history) is not a deletion; the current copy goes in its place.
//!
//! The hub may acknowledge less than it was sent: it holds back this program's writes of an object
//! another program's mark keeps away. `mark_synced` takes a prefix of the log, so from there on nothing
//! is marked, and those entries go again on the next sync, until the mark lifts and they are taken.

use fs_hub_proto::{ScanSize, images_of};
use fs_store::{Activity, ActivityFilter, Store};

use super::apply::HUB_ACTOR_PREFIX;
use super::redacted::with_current_copy;
use super::report::SyncReport;
use crate::push_plan::stub_images;
use crate::{HubSession, Result};

/// What may go of `a`, looking at the object as it is now.
async fn what_may_go(store: &dyn Store, a: Activity) -> Result<Option<Activity>> {
    let current = store.get(a.object).await?;
    let a = with_current_copy(a, current.as_ref());
    let marked = current
        .as_ref()
        .is_some_and(|o| !o.envelope.sensitive.is_empty());
    if a.kind.changes_state() {
        let hide = marked || (current.is_none() && was_marked_later(store, &a).await?);
        return Ok(Some(if hide { stub_images(a) } else { a }));
    }
    Ok((current.is_some() && !marked).then_some(a))
}

/// The object is gone: do its later unsynced entries say it was sensitive? One at a time, each may be
/// large.
async fn was_marked_later(store: &dyn Store, a: &Activity) -> Result<bool> {
    let mut after = a.id;
    loop {
        let later = store
            .activities(&ActivityFilter {
                after: Some(after),
                object: Some(a.object),
                unsynced_only: true,
                limit: Some(1),
                ..Default::default()
            })
            .await?;
        let Some(entry) = later.first() else {
            return Ok(false);
        };
        let (before, after_image) = images_of(entry).unwrap_or_default();
        if [before, after_image]
            .iter()
            .flatten()
            .any(|e| !e.sensitive.is_empty())
        {
            return Ok(true);
        }
        after = entry.id;
    }
}

pub(super) async fn push_all(
    hub: &HubSession,
    store: &dyn Store,
    report: &mut SyncReport,
) -> Result<()> {
    let mut scan = ScanSize::default();
    let (mut after, mut held_back) = (None, false);
    loop {
        let batch = store
            .activities(&ActivityFilter {
                after,
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
        let mut mark_up_to = (!held_back).then_some(batch_last);
        if let Some(send_last) = send.last().map(|a| a.id) {
            let handed_over = send.len();
            let outcome = hub.push(send).await?;
            report.pushed += handed_over - outcome.refused as usize;
            report.accepted += outcome.accepted;
            report.skipped += outcome.skipped;
            report.withheld += outcome.withheld;
            report.problems.extend(outcome.problems);
            // All refused here: the hub was not asked, and a hold stands as it was. Otherwise the hub
            // answers for all it was ever sent, so this can also end a hold.
            if outcome.refused as usize != handed_over {
                held_back = outcome.last_accepted.is_none_or(|acked| acked < send_last);
                mark_up_to = if held_back {
                    outcome.last_accepted
                } else {
                    Some(batch_last)
                };
            }
        }
        if let Some(up_to) = mark_up_to {
            store.mark_synced(up_to).await?;
        }
        after = Some(batch_last);
    }
}
