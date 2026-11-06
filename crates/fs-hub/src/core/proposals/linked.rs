//! Pairs a person already linked.
//!
//! Pattern: Specification input — `Relationship` objects (`owl:sameAs`) written by programs
//! after a human confirmed; those pairs are not proposed again.

use std::collections::BTreeSet;

use fs_store::traits::ObjectQuery;
use fs_store::{ObjectId, Query};

use crate::Result;

/// Ordered pairs `(smaller, larger)` already linked.
pub(super) async fn linked_pairs(
    objects: &dyn ObjectQuery,
) -> Result<BTreeSet<(ObjectId, ObjectId)>> {
    let mut out = BTreeSet::new();
    for r in objects.query(&Query::new().of_type("Relationship")).await? {
        let get = |k: &str| {
            r.index
                .get(k)
                .and_then(|v| v.first())
                .and_then(|s| s.parse::<ObjectId>().ok())
        };
        if let (Some(a), Some(b)) = (get("subject"), get("object")) {
            out.insert(if a < b { (a, b) } else { (b, a) });
        }
    }
    Ok(out)
}
