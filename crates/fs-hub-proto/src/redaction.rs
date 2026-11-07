//! Log entries that carry no object state.
//!
//! Pattern: Specification — a pure predicate over an activity. After `purge_object` (fs-store 0.4) later
//! writes of an object are logged as `{"redacted":true,"before":"redacted","after":"redacted"}`. Such an
//! entry is **not** a deletion: a consumer skips it and works from the object's current copy. When the hub
//! runs on fs-store 0.4, the body is `Activity::is_redacted()`; callers stay as they are.

use fs_store::Activity;

/// Does this entry carry no object state (purged history)? Never read `after == None` from it.
pub fn is_redacted(a: &Activity) -> bool {
    a.is_redacted()
}
