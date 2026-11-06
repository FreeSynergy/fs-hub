//! Pull step: hub -> local store.
//!
//! Pattern: Iterator-style batching — pulls until nothing more is waiting, applies each batch and
//! acknowledges to the hub. A bad entry is reported and passed; a failure that may pass stops here,
//! with the hub's cursor in front of the entry, so it comes again.

use fs_hub_proto::is_transient;
use fs_store::Store;

use super::apply::apply;
use super::report::SyncReport;
use crate::{Error, HubSession, Result};

const PULL_BATCH: u32 = 200;

pub(super) async fn pull_all(
    hub: &HubSession,
    store: &dyn Store,
    report: &mut SyncReport,
) -> Result<()> {
    loop {
        let mut batch = hub.pull(None, PULL_BATCH).await?;
        report.pulled += batch.activities.len();
        let (last, more) = (batch.last, batch.more);
        let empty = batch.activities.is_empty();
        let mut applied_up_to = None;
        for a in batch.activities {
            let (id, label) = (a.id, format!("{} {}", a.kind.as_str(), a.id));
            let note = batch.notes.remove(&id);
            match apply(store, a, note.as_ref()).await {
                Ok(changed) => report.applied += usize::from(changed),
                Err(Error::Store(e)) if is_transient(&e) => {
                    if let Some(done) = applied_up_to {
                        hub.ack(done).await?;
                    }
                    return Err(Error::Store(e));
                }
                Err(e) => report.failed.push(format!("{label}: {e}")),
            }
            applied_up_to = Some(id);
        }
        if let Some(last) = last {
            hub.ack(last).await?;
        }
        if !more || empty {
            return Ok(());
        }
    }
}
