//! One sync.
//!
//! Pattern: Template method — fixed skeleton (push all unsynced, pull all, push what is left), with the
//! steps in their own files. Whatever it learned comes along when it stops.

use fs_store::Store;

use super::report::SyncReport;
use super::{pull, push};
use crate::{Error, HubSession, Result};

pub(crate) async fn sync_once(hub: &HubSession, store: &dyn Store) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    match steps(hub, store, &mut report).await {
        Ok(()) => Ok(report),
        Err(cause) => Err(Error::Stopped {
            cause: Box::new(cause),
            report: Box::new(report),
        }),
    }
}

async fn steps(hub: &HubSession, store: &dyn Store, report: &mut SyncReport) -> Result<()> {
    push::push_all(hub, store, report).await?;
    pull::pull_all(hub, store, report).await?;
    if report.applied > 0 {
        // What was just applied is marked as passed, and what the program wrote meanwhile goes along.
        push::push_all(hub, store, report).await?;
    }
    Ok(())
}
