//! Content comparison shared by hub and client.
//!
//! Pattern: Specification — a small predicate ("is this the same object apart from store-owned
//! fields?") used by both sides to keep applying idempotent.

use chrono::{DateTime, Utc};
use fs_store::StoredObject;

/// Identical apart from the store-owned timestamps and the (immutable) original.
pub fn same_content(a: &StoredObject, b: &StoredObject) -> bool {
    let strip = |o: &StoredObject| {
        let mut o = o.clone();
        let epoch = DateTime::<Utc>::default();
        o.envelope.updated = epoch;
        o.envelope.published = epoch;
        o.original = None;
        o
    };
    strip(a) == strip(b)
}
