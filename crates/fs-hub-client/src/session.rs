//! The program's handle to a registered hub connection.
//!
//! Pattern: Facade — typed async methods hiding the command channel to the RPC thread.

use fs_store::{Activity, ActivityId};
use tokio::sync::{mpsc, oneshot};

use crate::command::Cmd;
use crate::push_plan;
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

    /// Program -> hub: hand over log entries (idempotent on the hub side). However much this is, it
    /// goes in pieces that fit a message. An entry above [`crate::MAX_OBJECT_BYTES`] does not leave
    /// the program: it is a problem in the outcome, and `last_accepted` moves past it.
    pub async fn push(&self, activities: Vec<Activity>) -> Result<PushOutcome> {
        let plan = push_plan::plan(activities);
        let mut outcome = plan.refused;
        if plan.pieces.is_empty() && outcome == PushOutcome::default() {
            return self.call(|r| Cmd::Push(vec![], r)).await;
        }
        for piece in plan.pieces {
            outcome.absorb(self.call(|r| Cmd::Push(piece, r)).await?);
        }
        Ok(outcome)
    }

    /// Hub -> program: other programs' activities (declared `reads` only). `None` = from the hub-kept
    /// cursor, `limit` 0 = the hub's default. An answer can hold fewer than `limit`: it is also cut by
    /// size, and `more` says there is more.
    pub async fn pull(&self, since: Option<ActivityId>, limit: u32) -> Result<PullBatch> {
        self.call(|r| Cmd::Pull(since, limit, false, r)).await
    }

    /// Like [`HubSession::pull`] without the content: every payload is `Null`, and
    /// [`PullBatch::notes`] holds what the object is. For programs that need the routing summary.
    pub async fn pull_envelopes(&self, since: Option<ActivityId>, limit: u32) -> Result<PullBatch> {
        self.call(|r| Cmd::Pull(since, limit, true, r)).await
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
    /// the hub-side subscription lives as long as the session. [`ChangeNotice::Ended`] says the hub
    /// ended the feed: subscribe again, then catch up with `pull`. The channel closing without it
    /// means the connection is gone.
    pub async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<ChangeNotice>> {
        self.call(Cmd::Subscribe).await
    }

    /// Push the unsynced log, pull what others wrote, mark synced. See [`crate::SyncReport`].
    pub async fn sync_once(&self, store: &dyn fs_store::Store) -> Result<crate::SyncReport> {
        crate::sync::sync_once(self, store).await
    }
}
