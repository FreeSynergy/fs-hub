//! Reading what other programs did without the content.
//!
//! The hub keeps one cursor per program, and it means "the content up to here is applied". A program
//! that reads only summaries must not move it, or `sync_once` would never get that content. So this
//! reader has its own place, a type that `HubSession::ack` does not take, and on the wire the hub refuses
//! an `ack` beyond what a session pulled with content:
//!
//! ```compile_fail
//! async fn wrong(hub: &fs_hub_client::HubSession, place: fs_hub_client::EnvelopeCursor) {
//!     hub.ack(place).await.unwrap();
//! }
//! ```
//!
//! Pattern: Newtype — [`EnvelopeCursor`] keeps the two kinds of place apart.

use std::fmt;
use std::str::FromStr;

use fs_hub_proto::PullBatch;
use fs_store::{ActivityId, ActivityKind, Envelope, ObjectId, Timestamp};

/// A place in the hub's log for a reader of summaries. Keep it to go on, also as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EnvelopeCursor(ActivityId);

impl EnvelopeCursor {
    pub(crate) fn activity(self) -> ActivityId {
        self.0
    }
}

impl fmt::Display for EnvelopeCursor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for EnvelopeCursor {
    type Err = <ActivityId as FromStr>::Err;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

/// What one activity was about.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeEntry {
    /// The place of this entry.
    pub place: EnvelopeCursor,
    /// What happened.
    pub kind: ActivityKind,
    /// The object.
    pub object: ObjectId,
    /// The program that did it.
    pub program: String,
    /// When the hub applied it.
    pub at: Timestamp,
    /// Summary of the object, without the fields a reader never gets.
    pub envelope: Envelope,
    /// When the program did it.
    pub original_at: Option<Timestamp>,
}

/// One answer of `pull_envelopes`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnvelopeBatch {
    /// The entries, in log order.
    pub entries: Vec<EnvelopeEntry>,
    /// Hand this to the next `pull_envelopes`.
    pub next: Option<EnvelopeCursor>,
    /// More are waiting.
    pub more: bool,
}

impl EnvelopeBatch {
    pub(crate) fn from_pull(mut batch: PullBatch) -> Self {
        let entries = batch
            .activities
            .into_iter()
            .filter_map(|a| {
                let note = batch.notes.remove(&a.id)?;
                Some(EnvelopeEntry {
                    place: EnvelopeCursor(a.id),
                    kind: a.kind,
                    object: a.object,
                    program: a.actor,
                    at: a.at,
                    envelope: note.envelope,
                    original_at: note.original_at,
                })
            })
            .collect();
        Self {
            entries,
            next: batch.last.map(EnvelopeCursor),
            more: batch.more,
        }
    }
}
