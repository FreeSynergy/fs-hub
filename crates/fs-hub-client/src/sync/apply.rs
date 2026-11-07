//! Applying one pulled activity to the program's store.
//!
//! Pattern: Command (executed) — create/update/delete/log-only with the actor written as
//! `hub:<program>`; identical content is skipped (idempotent). What the hub blanks at delivery stays
//! as this store has it: an update from a reader must not take the owner's values away. And what is
//! marked sensitive here stays as it is: a mark is this program's own, nothing pulled undoes it.

use fs_hub_proto::{keep_held_back, same_content};
use fs_objects::{Activity, ActivityKind, ChangeSet, ObjectId, Store};

use crate::{Error, Result};

/// Log entries written by [`apply`] start with this. They came from the hub, so they never go back.
pub(super) const HUB_ACTOR_PREFIX: &str = "hub:";

/// Apply one pulled activity to the program's store. Returns whether that changed the store.
pub(super) async fn apply(store: &dyn Store, a: Activity) -> Result<bool> {
    let actor = format!("{HUB_ACTOR_PREFIX}{}", a.actor);
    if a.payload.is_null() && a.kind == ActivityKind::Delete {
        return delete(store, a.object, &actor).await;
    }
    if !a.kind.changes_state() {
        if store.get(a.object).await?.is_none() {
            return Ok(false);
        }
        store.log(a.kind, a.object, &actor, a.payload).await?;
        return Ok(true);
    }
    if a.payload.is_null() {
        return Err(Error::Protocol(
            "the hub held back the content: too large for a message".into(),
        ));
    }
    let object = a.object;
    let cs: ChangeSet = serde_json::from_value(a.payload).map_err(fs_objects::Error::from)?;
    let Some(mut obj) = cs.after else {
        return delete(store, object, &actor).await;
    };
    if let Some(have) = store.get(obj.envelope.id).await? {
        if !have.envelope.sensitive.is_empty() || same_content(&have, &obj) {
            return Ok(false);
        }
        keep_held_back(&have, &mut obj);
    }
    store.put(obj, &actor).await?;
    Ok(true)
}

async fn delete(store: &dyn Store, object: ObjectId, actor: &str) -> Result<bool> {
    let Some(have) = store.get(object).await? else {
        return Ok(false);
    };
    if !have.envelope.sensitive.is_empty() {
        return Ok(false);
    }
    store.delete(object, actor).await?;
    Ok(true)
}
