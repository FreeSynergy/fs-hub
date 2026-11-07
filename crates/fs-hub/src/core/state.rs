//! What the hub remembers per program.
//!
//! Pattern: Value object — plain serialisable state (manifest + cursors); persistence is the job
//! of a `CursorRepository`.

use fs_hub_proto::Manifest;
use fs_objects::{ActivityId, ObjectId};
use serde::{Deserialize, Serialize};

/// Per-program state; cursors survive restarts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgramState {
    /// Id of the store object holding this state.
    pub object_id: ObjectId,
    /// The program's latest manifest.
    pub manifest: Manifest,
    /// Last activity of the program's own log the hub applied (idempotency + ack).
    pub last_accepted: Option<ActivityId>,
    /// Last hub activity the program acknowledged (`ack`).
    pub pull_cursor: Option<ActivityId>,
    /// Last contact, unix milliseconds.
    pub last_seen_ms: i64,
    /// Writes of objects this program first wrote, while another program's mark keeps them away.
    #[serde(default)]
    pub held: Vec<HeldWrites>,
    /// When the stores' logs were last compacted, unix milliseconds; not itself persisted.
    #[serde(skip)]
    pub settled_ms: i64,
}

/// A program's entries about one object that the hub took no notice of yet, and must see again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldWrites {
    /// The object.
    pub object: ObjectId,
    /// Its first held entry.
    pub first: ActivityId,
    /// Its last held entry.
    pub last: ActivityId,
    /// What the program could mark synced at that first entry.
    pub acked_before: Option<ActivityId>,
}

impl ProgramState {
    /// What the program may mark synced: `mark_synced` takes a prefix of its log, so this stops in
    /// front of the first held entry, or the program would count it as delivered.
    pub fn acknowledged(&self) -> Option<ActivityId> {
        self.held
            .iter()
            .min_by_key(|h| h.first)
            .map_or(self.last_accepted, |h| h.acked_before)
    }

    /// Must this entry be looked at again, though the cursor is past it?
    pub(super) fn is_held(&self, object: ObjectId, entry: ActivityId) -> bool {
        self.held
            .iter()
            .any(|h| h.object == object && h.first <= entry && entry <= h.last)
    }

    /// The hub could not take this entry yet.
    pub(super) fn hold(&mut self, object: ObjectId, entry: ActivityId) {
        match self.held.iter_mut().find(|h| h.object == object) {
            Some(h) => h.last = h.last.max(entry),
            None => self.held.push(HeldWrites {
                object,
                first: entry,
                last: entry,
                acked_before: self.last_accepted,
            }),
        }
    }

    /// The hub took this entry: once that reaches the last held one, nothing is held for the object.
    pub(super) fn release(&mut self, object: ObjectId, entry: ActivityId) {
        self.held.retain(|h| h.object != object || entry < h.last);
    }
}

/// Now, unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
