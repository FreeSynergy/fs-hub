//! Turning shared keys into proposal pairs.
//!
//! Pattern: Pure function — candidates per key in, de-duplicated proposals out (different
//! programs, same kind, not linked yet).

use std::collections::{BTreeMap, BTreeSet};

use fs_hub_proto::Proposal;
use fs_store::ObjectId;

use super::candidates::Candidate;

/// One proposal per pair, with every shared key as a reason.
pub(super) fn pair_up(
    by_value: BTreeMap<String, Vec<Candidate>>,
    linked: &BTreeSet<(ObjectId, ObjectId)>,
) -> Vec<Proposal> {
    let mut pairs: BTreeMap<(ObjectId, ObjectId), Proposal> = BTreeMap::new();
    for (reason, items) in by_value {
        for (i, x) in items.iter().enumerate() {
            for y in &items[i + 1..] {
                if x.id == y.id || x.kind != y.kind || x.origin == y.origin {
                    continue;
                }
                let (first, second) = if x.id < y.id { (x, y) } else { (y, x) };
                let key = (first.id, second.id);
                if linked.contains(&key) {
                    continue;
                }
                let p = pairs.entry(key).or_insert_with(|| Proposal {
                    a: first.id,
                    b: second.id,
                    name_a: first.name.clone(),
                    name_b: second.name.clone(),
                    program_a: first.origin.clone(),
                    program_b: second.origin.clone(),
                    reasons: vec![],
                });
                p.reasons.push(reason.clone());
            }
        }
    }
    pairs.into_values().collect()
}
