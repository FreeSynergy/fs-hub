//! Applying one pulled activity to the program's store.
//!
//! Pattern: Command (executed) — create/update/delete/log-only with the actor written as
//! `hub:<program>`; identical content is skipped (idempotent).

use fs_hub_proto::same_content;
use fs_store::{Activity, ActivityId, Store};

use crate::{Error, Result};

/// Apply one pulled activity to the program's store. Returns the ids of the log entries it created.
pub(super) async fn apply(store: &dyn Store, a: &Activity) -> Result<Vec<ActivityId>> {
    let actor = format!("hub:{}", a.actor);
    if !a.kind.changes_state() {
        if store.get(a.object).await?.is_none() {
            return Ok(vec![]);
        }
        let act = store
            .log(a.kind, a.object, &actor, a.payload.clone())
            .await?;
        return Ok(vec![act.id]);
    }
    if a.payload.is_null() {
        return Err(Error::Protocol(
            "the hub held back the content: too large for a message".into(),
        ));
    }
    let cs = a.change_set().map_err(Error::Store)?;
    match cs.after {
        Some(obj) => {
            if let Some(have) = store.get(obj.envelope.id).await?
                && same_content(&have, &obj)
            {
                return Ok(vec![]);
            }
            Ok(vec![store.put(obj, &actor).await?.id])
        }
        None => {
            if store.get(a.object).await?.is_some() {
                Ok(vec![store.delete(a.object, &actor).await?.id])
            } else {
                Ok(vec![])
            }
        }
    }
}
