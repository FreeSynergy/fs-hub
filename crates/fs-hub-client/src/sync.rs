//! `sync_once`: the program's side of ADR 0019 point 4 (activity log in both directions).

use std::collections::BTreeSet;

use fs_store::{Activity, ActivityFilter, ActivityId, Store, StoredObject};

use crate::{Error, HubSession, Result};

const PUSH_BATCH: usize = 100;
const PULL_BATCH: u32 = 200;

/// What one sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Log entries sent.
    pub pushed: usize,
    /// ... of which the hub applied.
    pub accepted: u32,
    /// ... already known to the hub.
    pub skipped: u32,
    /// ... not collected (sensitive, ADR 0029; or not a declared `writes` type).
    pub withheld: u32,
    /// Problems the hub reported for single entries.
    pub problems: Vec<String>,
    /// Activities received from other programs.
    pub pulled: usize,
    /// ... changing the local store.
    pub applied: usize,
    /// ... that could not be applied (reason).
    pub failed: Vec<String>,
}

pub(crate) async fn sync_once(hub: &HubSession, store: &dyn Store) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    push_all(hub, store, &mut report).await?;
    pull_all(hub, store, &mut report).await?;
    Ok(report)
}

async fn push_all(hub: &HubSession, store: &dyn Store, report: &mut SyncReport) -> Result<()> {
    loop {
        let batch = store
            .activities(&ActivityFilter { unsynced_only: true, limit: Some(PUSH_BATCH), ..Default::default() })
            .await?;
        let Some(batch_last) = batch.last().map(|a| a.id) else { return Ok(()) };
        report.pushed += batch.len();
        let outcome = hub.push(batch).await?;
        report.accepted += outcome.accepted;
        report.skipped += outcome.skipped;
        report.withheld += outcome.withheld;
        report.problems.extend(outcome.problems);
        let acked = outcome.last_accepted.ok_or_else(|| Error::Protocol("hub acknowledged nothing".into()))?;
        if acked < batch_last {
            // Would loop forever; the hub's cursor is behind what it just processed.
            return Err(Error::Protocol(format!("hub acknowledged {acked}, expected at least {batch_last}")));
        }
        store.mark_synced(batch_last).await?;
    }
}

async fn pull_all(hub: &HubSession, store: &dyn Store, report: &mut SyncReport) -> Result<()> {
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
                Err(e) => report.failed.push(format!("{} {}: {e}", a.kind.as_str(), a.id)),
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

/// What came from the hub must not travel back as new work: mark the log entries our own `apply`
/// wrote as synced. `mark_synced` acknowledges a prefix of the log, so entries after the first
/// foreign unsynced one stay (the hub drops them as duplicates of what it already holds).
async fn mark_applied_synced(store: &dyn Store, ours: &BTreeSet<ActivityId>) -> Result<()> {
    let Some(&max_ours) = ours.last() else { return Ok(()) };
    let unsynced = store.activities(&ActivityFilter { unsynced_only: true, ..Default::default() }).await?;
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

/// Identical apart from the store-owned timestamps and the (immutable) original.
fn same_content(a: &StoredObject, b: &StoredObject) -> bool {
    let strip = |o: &StoredObject| {
        let mut o = o.clone();
        let epoch = chrono::DateTime::<chrono::Utc>::default();
        o.envelope.updated = epoch;
        o.envelope.published = epoch;
        o.original = None;
        o
    };
    strip(a) == strip(b)
}

/// Apply one pulled activity to the program's store. Returns the ids of the log entries it created.
async fn apply(store: &dyn Store, a: &Activity) -> Result<Vec<ActivityId>> {
    let actor = format!("hub:{}", a.actor);
    if !a.kind.changes_state() {
        if store.get(a.object).await?.is_none() {
            return Ok(vec![]);
        }
        let act = store.log(a.kind, a.object, &actor, a.payload.clone()).await?;
        return Ok(vec![act.id]);
    }
    let cs = a.change_set().map_err(Error::Store)?;
    match cs.after {
        Some(obj) => {
            if let Some(have) = store.get(obj.envelope.id).await? {
                if same_content(&have, &obj) {
                    return Ok(vec![]);
                }
            }
            Ok(vec![store.put(obj, &actor).await?.id])
        }
        None => {
            if store.get(a.object).await?.is_some() {
                Ok(vec![store.delete(a.object, &actor).await?.id])
            } else {
                Ok(vec![])
            }
        }
    }
}
