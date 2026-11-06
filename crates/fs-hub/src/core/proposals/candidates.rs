//! Candidate extraction: which profiles carry which identity keys.
//!
//! Pattern: Iterator-style pipeline — profiles in, `key -> [candidate]` out (groups and
//! sensitive objects skipped).

use std::collections::BTreeMap;

use fs_store::traits::{ActivityLog, ObjectQuery};
use fs_store::{ActivityFilter, ObjectId, Query};

use crate::Result;

/// One profile that has an identity key.
pub(super) struct Candidate {
    pub id: ObjectId,
    pub kind: String,
    pub origin: String,
    pub name: String,
}

/// Identity keys (`email:...`, `phone:...`) -> profiles having them.
pub(super) async fn by_identity_key(
    objects: &dyn ObjectQuery,
    log: &dyn ActivityLog,
) -> Result<BTreeMap<String, Vec<Candidate>>> {
    let mut by_value: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for o in objects.query(&Query::new().of_type("Profile")).await? {
        if !o.envelope.sensitive.is_empty() {
            continue;
        }
        let kind = o
            .index
            .get("kind")
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_else(|| "individual".into());
        if kind == "group" {
            continue;
        }
        let origin = origin_of(log, o.envelope.id).await?;
        let mut keys = vec![];
        keys.extend(
            o.index
                .get("email")
                .into_iter()
                .flatten()
                .map(|e| format!("email:{e}")),
        );
        keys.extend(
            o.index
                .get("phone")
                .into_iter()
                .flatten()
                .filter(|p| p.trim_start_matches('+').len() >= 6)
                .map(|p| format!("phone:{p}")),
        );
        for k in keys {
            by_value.entry(k).or_default().push(Candidate {
                id: o.envelope.id,
                kind: kind.clone(),
                origin: origin.clone(),
                name: o.envelope.name.clone(),
            });
        }
    }
    Ok(by_value)
}

/// The program that first wrote the object into the hub.
async fn origin_of(log: &dyn ActivityLog, id: ObjectId) -> Result<String> {
    let first = log
        .activities(&ActivityFilter {
            object: Some(id),
            limit: Some(1),
            ..Default::default()
        })
        .await?;
    Ok(first.first().map(|a| a.actor.clone()).unwrap_or_default())
}
