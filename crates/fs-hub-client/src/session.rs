//! The program's handle to a registered hub connection.
//!
//! Pattern: Facade — typed async methods hiding the command channel to the RPC thread.

use fs_store::{Activity, ActivityId};
use tokio::sync::{mpsc, oneshot};

use crate::command::Cmd;
use crate::{ChangeNotice, Error, ProgramInfo, Proposal, PullBatch, PushOutcome, Result};

/// A registered connection to the hub. Cheap to clone; the connection closes when the last clone is dropped.
#[derive(Clone)]
pub struct HubSession {
    cmds: mpsc::UnboundedSender<Cmd>,
}

impl HubSession {
    pub(crate) fn new(cmds: mpsc::UnboundedSender<Cmd>) -> Self {
        Self { cmds }
    }

    async fn call<T>(&self, make: impl FnOnce(oneshot::Sender<Result<T>>) -> Cmd) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmds.send(make(tx)).map_err(|_| Error::Closed)?;
        rx.await.map_err(|_| Error::Closed)?
    }

    /// Program -> hub: hand over log entries (idempotent on the hub side).
    pub async fn push(&self, activities: Vec<Activity>) -> Result<PushOutcome> {
        self.call(|r| Cmd::Push(activities, r)).await
    }

    /// Hub -> program: other programs' activities (declared `reads` only). `None` = from the hub-kept cursor.
    pub async fn pull(&self, since: Option<ActivityId>, limit: u32) -> Result<PullBatch> {
        self.call(|r| Cmd::Pull(since, limit, r)).await
    }

    /// Tell the hub everything up to `up_to` was applied.
    pub async fn ack(&self, up_to: ActivityId) -> Result<()> {
        self.call(|r| Cmd::Ack(up_to, r)).await
    }

    /// The programs the hub knows.
    pub async fn programs(&self) -> Result<Vec<ProgramInfo>> {
        self.call(Cmd::Programs).await
    }

    /// The hub's same-person candidates (never sensitive data).
    pub async fn proposals(&self) -> Result<Vec<Proposal>> {
        self.call(Cmd::Proposals).await
    }

    /// Live notices about other programs' changes (same filter as `pull`). Drop the receiver to stop caring;
    /// the hub-side subscription lives as long as the session.
    pub async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ChangeNotice>> {
        self.call(Cmd::Subscribe).await
    }

    /// Push the unsynced log, pull what others wrote, mark synced. See [`crate::SyncReport`].
    pub async fn sync_once(&self, store: &dyn fs_store::Store) -> Result<crate::SyncReport> {
        crate::sync::sync_once(self, store).await
    }
}
