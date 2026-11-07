//! Content comparison shared by hub and client.
//!
//! Pattern: Specification — small predicates ("is this the same object apart from ...?") used by both
//! sides to keep applying idempotent.

use chrono::{DateTime, Utc};
use fs_objects::StoredObject;

use crate::blank_object;

fn without_store_fields(o: &StoredObject) -> StoredObject {
    let mut o = o.clone();
    let epoch = DateTime::<Utc>::default();
    o.envelope.updated = epoch;
    o.envelope.published = epoch;
    o.original = None;
    o
}

/// Identical apart from what the store owns and what a reader never gets: a reader's copy lacks those,
/// and that is no change.
pub fn same_content(a: &StoredObject, b: &StoredObject) -> bool {
    let strip = |o: &StoredObject| {
        let mut o = without_store_fields(o);
        blank_object(&mut o);
        o
    };
    strip(a) == strip(b)
}

/// Identical apart from what the store owns. For the program that wrote the object: what it changes in
/// the fields a reader never gets is a change.
pub fn same_as_written(a: &StoredObject, b: &StoredObject) -> bool {
    without_store_fields(a) == without_store_fields(b)
}
