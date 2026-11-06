//! Content comparison shared by hub and client.
//!
//! Pattern: Specification — a small predicate ("is this the same object apart from what the store owns
//! and what a reader never gets?") used by both sides to keep applying idempotent.

use chrono::{DateTime, Utc};
use fs_store::StoredObject;

use crate::blank_for_delivery;

/// Identical apart from the store-owned timestamps, the (immutable) original and the fields blanked at
/// delivery: a reader's copy lacks those, and that is no change.
pub fn same_content(a: &StoredObject, b: &StoredObject) -> bool {
    let strip = |o: &StoredObject| {
        let mut o = o.clone();
        let epoch = DateTime::<Utc>::default();
        o.envelope.updated = epoch;
        o.envelope.published = epoch;
        blank_for_delivery(&mut o.envelope);
        o.original = None;
        o
    };
    strip(a) == strip(b)
}
