//! Activities on the wire.
//!
//! Pattern: Adapter — `fs_store::Activity` <-> `hub.capnp` `Activity`; the typed envelope is the routing summary, the JSON payload carries the full images.

use fs_store::{Activity, ActivityKind, Envelope, ObjectId};

use super::envelope::write_envelope;
use super::ids::{from_ms, ms, read_activity_id, read_uuid, write_activity_id, write_uuid};
use crate::hub_capnp as hub;
use crate::{Error, Result};

/// The object envelope an activity is about (after image, else before image), if it carries one.
fn summary_of(a: &Activity) -> Option<Envelope> {
    if !a.kind.changes_state() {
        return None;
    }
    let cs = a.change_set().ok()?;
    cs.after.or(cs.before).map(|o| o.envelope)
}

/// Store activity -> wire.
pub fn write_activity(mut b: hub::activity::Builder<'_>, a: &Activity) -> Result<()> {
    write_activity_id(b.reborrow().init_id(), Some(a.id));
    b.set_kind(a.kind.as_str());
    write_uuid(b.reborrow().init_object(), Some(a.object.as_uuid()));
    b.set_actor(&a.actor);
    b.reborrow().init_at().set_unix_millis(ms(a.at));
    b.set_has_target(a.target.is_some());
    write_activity_id(b.reborrow().init_target(), a.target);
    match summary_of(a) {
        Some(env) => write_envelope(b.reborrow().init_envelope(), &env),
        None => write_uuid(
            b.reborrow().init_envelope().init_id(),
            Some(a.object.as_uuid()),
        ),
    }
    b.set_payload(&serde_json::to_vec(&a.payload)?);
    Ok(())
}

/// Wire activity -> store activity (`synced` is false: it is not in the receiver's log).
pub fn read_activity(r: hub::activity::Reader<'_>) -> Result<Activity> {
    let id = read_activity_id(r.get_id()?)?
        .ok_or_else(|| Error::Invalid("activity without id".into()))?;
    let object = read_uuid(r.get_object()?)
        .ok_or_else(|| Error::Invalid("activity without object".into()))?;
    let kind: ActivityKind = r.get_kind()?.to_str()?.parse()?;
    let target = if r.get_has_target() {
        read_activity_id(r.get_target()?)?
    } else {
        None
    };
    Ok(Activity {
        id,
        kind,
        object: ObjectId::from_uuid(object)?,
        actor: r.get_actor()?.to_string()?,
        at: from_ms(r.get_at()?.get_unix_millis()),
        target,
        payload: serde_json::from_slice(r.get_payload()?)?,
        synced: false,
    })
}

/// A whole list of activities -> wire.
pub fn write_activities(
    mut list: capnp::struct_list::Builder<'_, hub::activity::Owned>,
    items: &[Activity],
) -> Result<()> {
    for (i, a) in items.iter().enumerate() {
        write_activity(list.reborrow().get(i as u32), a)?;
    }
    Ok(())
}

/// A whole wire list -> store activities.
pub fn read_activities(
    list: capnp::struct_list::Reader<'_, hub::activity::Owned>,
) -> Result<Vec<Activity>> {
    list.iter().map(read_activity).collect()
}
