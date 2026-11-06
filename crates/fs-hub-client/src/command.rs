//! Requests to the RPC thread.
//!
//! Pattern: Command — each session call is an enum value carrying its arguments and a reply
//! channel; the worker thread executes them one by one.

use fs_hub_proto::{ChangeNotice, ProgramInfo, Proposal, PullBatch, PushOutcome};
use fs_store::{Activity, ActivityId};
use tokio::sync::{mpsc, oneshot};

use crate::Result;

/// Reply channel of a command.
pub(crate) type Reply<T> = oneshot::Sender<Result<T>>;

/// One request to the worker.
pub(crate) enum Cmd {
    Push(Vec<Activity>, Reply<PushOutcome>),
    Pull(Option<ActivityId>, u32, Reply<PullBatch>),
    Ack(ActivityId, Reply<()>),
    Programs(Reply<Vec<ProgramInfo>>),
    Proposals(Reply<Vec<Proposal>>),
    Subscribe(Reply<mpsc::UnboundedReceiver<ChangeNotice>>),
}
