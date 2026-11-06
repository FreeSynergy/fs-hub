//! Activities on the wire.
//!
//! Pattern: Adapter — `fs_store::Activity` <-> `hub.capnp` `Activity`, one form per direction: a push
//! says when the program did it, an answer of `pull` carries what the hub knows besides the log entry.
//! The typed envelope is the routing summary, the JSON payload carries the full images; a `Null`
//! payload stays empty on the wire. `originalActor` is written by nobody: only the person can release
//! it, and no manifest can (see `DESIGN.md`).

use std::collections::BTreeMap;

use fs_interfaces::rpc::{read_envelope, write_envelope};
use fs_store::{Activity, ActivityId, ActivityKind, Envelope, ObjectId};

use super::ids::{from_ms, ms, read_activity_id, read_uuid, write_activity_id, write_uuid};
use super::images::images_of;
use crate::hub_capnp as hub;
use crate::{Error, PullNote, Pushed, Result};

fn write_activity(
    mut b: hub::activity::Builder<'_>,
    a: &Activity,
    actor: &str,
    summary: Option<&Envelope>,
    original_at: Option<fs_store::Timestamp>,
) -> Result<()> {
    write_activity_id(b.reborrow().init_id(), Some(a.id));
    b.set_kind(a.kind.as_str());
    write_uuid(b.reborrow().init_object(), Some(a.object.as_uuid()));
    b.set_actor(actor);
    b.reborrow().init_at().set_unix_millis(ms(a.at));
    b.set_has_target(a.target.is_some());
    write_activity_id(b.reborrow().init_target(), a.target);
    match summary {
        Some(env) => write_envelope(b.reborrow().init_envelope(), env),
        None => write_uuid(
            b.reborrow().init_envelope().init_id(),
            Some(a.object.as_uuid()),
        ),
    }
    if !a.payload.is_null() {
        b.set_payload(&serde_json::to_vec(&a.payload)?);
    }
    if let Some(t) = original_at {
        b.init_original_at().set_unix_millis(ms(t));
    }
    Ok(())
}

fn read_activity(r: hub::activity::Reader<'_>) -> Result<Activity> {
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
    let payload = r.get_payload()?;
    Ok(Activity {
        id,
        kind,
        object: ObjectId::from_uuid(object)?,
        actor: r.get_actor()?.to_string()?,
        at: from_ms(r.get_at()?.get_unix_millis()),
        target,
        payload: if payload.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(payload)?
        },
        synced: false,
    })
}

fn read_original_at(r: hub::activity::Reader<'_>) -> Result<Option<fs_store::Timestamp>> {
    if !r.has_original_at() {
        return Ok(None);
    }
    Ok(Some(from_ms(r.get_original_at()?.get_unix_millis())))
}

/// Log entries of a program -> wire, each with the time the program did it. A sensitive object never
/// says when (ADR 0029), also when only the before image is sensitive. `actor` stays blank: who did
/// it stays in the program's log.
pub fn write_pushed_list(
    mut list: capnp::struct_list::Builder<'_, hub::activity::Owned>,
    items: &[Activity],
) -> Result<()> {
    for (i, a) in items.iter().enumerate() {
        let (before, after) = images_of(a).unwrap_or_default();
        let is_sensitive =
            |e: &Option<Envelope>| e.as_ref().is_some_and(|e| !e.sensitive.is_empty());
        let original_at = (!is_sensitive(&before) && !is_sensitive(&after)).then_some(a.at);
        write_activity(
            list.reborrow().get(i as u32),
            a,
            "",
            after.or(before).as_ref(),
            original_at,
        )?;
    }
    Ok(())
}

/// Wire -> what a program handed over.
pub fn read_pushed_list(
    list: capnp::struct_list::Reader<'_, hub::activity::Owned>,
) -> Result<Vec<Pushed>> {
    list.iter()
        .map(|r| {
            Ok(Pushed {
                activity: read_activity(r)?,
                original_at: read_original_at(r)?,
            })
        })
        .collect()
}

/// An answer of `pull` -> wire. An activity without a note goes without a summary.
pub fn write_pulled_list(
    mut list: capnp::struct_list::Builder<'_, hub::activity::Owned>,
    items: &[Activity],
    notes: &BTreeMap<ActivityId, PullNote>,
) -> Result<()> {
    for (i, a) in items.iter().enumerate() {
        let note = notes.get(&a.id);
        write_activity(
            list.reborrow().get(i as u32),
            a,
            &a.actor,
            note.map(|n| &n.envelope),
            note.and_then(|n| n.original_at),
        )?;
    }
    Ok(())
}

type Pulled = (Vec<Activity>, BTreeMap<ActivityId, PullNote>);

/// Wire -> an answer of `pull`.
pub(super) fn read_pulled_list(
    list: capnp::struct_list::Reader<'_, hub::activity::Owned>,
) -> Result<Pulled> {
    let mut activities = Vec::with_capacity(list.len() as usize);
    let mut notes = BTreeMap::new();
    for r in list {
        let a = read_activity(r)?;
        let wire = r.get_envelope()?;
        if wire.has_published() {
            let note = PullNote {
                envelope: read_envelope(wire)?,
                original_at: read_original_at(r)?,
            };
            notes.insert(a.id, note);
        }
        activities.push(a);
    }
    Ok((activities, notes))
}
