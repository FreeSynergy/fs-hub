//! What goes to the hub for an entry that carries no state.
//!
//! Pattern: Template method (one step of the push) — after `purge_object` (fs-store 0.4) later writes of
//! an object are logged without images. That is not a deletion. The entry keeps its id (the hub's
//! acknowledgement counts ids), but says what the object is now: the current copy, or a delete when the
//! store no longer has it. The marked and the sensitive rules of `push.rs` then apply to that as usual.

use fs_hub_proto::is_redacted;
use fs_store::{Activity, ActivityKind, StoredObject};

/// `a` as the hub can read it: untouched unless it is redacted, then the images are the store's
/// `current` copy (as `after`) and the kind says what that means.
pub(super) fn with_current_copy(mut a: Activity, current: Option<&StoredObject>) -> Activity {
    if !a.kind.changes_state() || !is_redacted(&a) {
        return a;
    }
    a.kind = if current.is_some() {
        ActivityKind::Update
    } else {
        ActivityKind::Delete
    };
    a.payload = serde_json::json!({ "before": null, "after": current });
    a
}

#[cfg(test)]
mod tests;
