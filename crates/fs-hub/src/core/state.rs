//! What the hub remembers per program.
//!
//! Pattern: Value object — plain serialisable state (manifest + cursors); persistence is the job
//! of a `CursorRepository`.

use fs_hub_proto::Manifest;
use fs_store::{ActivityId, ObjectId};
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
    /// When this was last persisted, unix milliseconds; not itself persisted.
    #[serde(skip)]
    pub saved_ms: i64,
}

/// Now, unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
