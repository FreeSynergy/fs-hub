//! What a reader does not get of an object, and what a sensitive object says at all.
//!
//! PROVISIONAL (strategy session, 2026-11-06): the cores put the person's IRI into `attributedTo` and
//! `to`, and an rfc822 original or an index key can name people as well, so these would tell every
//! reader who did it, past the empty `originalActor`. Until the person can release them, a reader gets
//! neither these fields nor the original nor such index keys. [`held_back`] and
//! [`DELIVERED_INDEX_KEYS`] are the only lists of them, and [`blank_for_delivery`] with
//! [`blank_object`] the only place that blanks them: a later release per person and per program decides
//! here. The hub and the owner keep the values: a reader that pushes a blanked object back overwrites
//! nothing ([`keep_held_back`]), and it is no change ([`crate::same_content`]).
//!
//! Pattern: Specification — one rule, used by every way out of the hub.

use chrono::{DateTime, Utc};
use fs_store::{Envelope, ObjectId, StoredObject};
use serde::Deserialize;
use serde_json::Value;

/// The `sensitive` entry of a stub: it says that, not which category.
pub const SENSITIVE_MARKER: &str = "sensitive";

const STUB_TYPE: &str = "Object";
const STUB_NAME: &str = "withheld";

/// Index keys only a card has for a reader: they say what its content says, which is the object itself.
/// So they go to programs allowed to read the card type anyway, never of a marked card, as that is not
/// delivered at all (ADR 0029 point 1), and on no other type.
const PROFILE_INDEX_KEYS: &[&str] = &["email", "nickname", "org", "phone"];

/// On a same-person link `subject` and `object` are card ids, not people's IRIs. Other programs use
/// `subject` for other things, so only here.
const RELATIONSHIP_INDEX_KEYS: &[&str] = &["subject"];

/// Index keys a reader gets of any object: what it needs to find and handle it. Anything else does not
/// go, so a new key naming people stays home until somebody decides otherwise (`person`, `member`,
/// `holder`, `invitee`, `participant_email`, ...).
pub const DELIVERED_INDEX_KEYS: &[&str] = &[
    "activity",
    "backend",
    "directory",
    "folder",
    "group",
    "key",
    "kind",
    "label",
    "link",
    "name_exact",
    "name_sort",
    "need",
    "object",
    "parent",
    "path",
    "progress",
    "relationship",
    "removed",
    "reservation",
    "resource",
    "right",
    "role",
    "role_id",
    "scope",
    "stage",
    "state",
    "submitted",
    "survey",
    "task",
    "trashed",
    "uid",
];

/// The envelope fields a reader does not get. The only list of them.
fn held_back(e: &mut Envelope) -> (&mut Option<String>, [&mut Vec<String>; 5]) {
    (
        &mut e.attributed_to,
        [
            &mut e.to,
            &mut e.cc,
            &mut e.bto,
            &mut e.bcc,
            &mut e.audience,
        ],
    )
}

fn is_delivered(index_key: &str, object_type: &str) -> bool {
    DELIVERED_INDEX_KEYS.contains(&index_key)
        || (object_type == "Profile" && PROFILE_INDEX_KEYS.contains(&index_key))
        || (object_type == "Relationship" && RELATIONSHIP_INDEX_KEYS.contains(&index_key))
}

/// Blank what a reader does not get of an envelope. Everything leaving the hub goes through this.
pub fn blank_for_delivery(e: &mut Envelope) {
    let (attributed_to, lists) = held_back(e);
    *attributed_to = None;
    for list in lists {
        list.clear();
    }
}

/// [`blank_for_delivery`], and what a reader does not get of the rest of an object.
pub fn blank_object(o: &mut StoredObject) {
    blank_for_delivery(&mut o.envelope);
    o.original = None;
    let object_type = o.envelope.object_type.as_str();
    o.index.retain(|key, _| is_delivered(key, object_type));
}

/// `into` came from a reader, or from the hub to a reader: what it has blank takes the value in `from`.
/// So a blanked object going round overwrites nothing. The original is the store's: it keeps it.
pub fn keep_held_back(from: &StoredObject, into: &mut StoredObject) {
    let mut envelope = from.envelope.clone();
    let (from_attributed, from_lists) = held_back(&mut envelope);
    let (attributed, lists) = held_back(&mut into.envelope);
    if attributed.is_none() {
        *attributed = from_attributed.take();
    }
    for (list, from_list) in lists.into_iter().zip(from_lists) {
        if list.is_empty() {
            std::mem::swap(list, from_list);
        }
    }
    for (key, values) in &from.index {
        if !is_delivered(key, &into.envelope.object_type) {
            into.index
                .entry(key.clone())
                .or_insert_with(|| values.clone());
        }
    }
}

/// [`blank_object`] on the before/after images, in place: the content is neither touched nor copied.
/// What cannot be read does not go out: the payload is then empty, as for one too large.
pub fn blank_images(payload: &mut Value) {
    for name in ["before", "after"] {
        let Some(image) = payload.get_mut(name).filter(|i| i.is_object()) else {
            continue;
        };
        if blank_image(image).is_none() {
            *payload = Value::Null;
            return;
        }
    }
}

fn blank_image(image: &mut Value) -> Option<()> {
    let envelope = Envelope::deserialize(image.get("envelope")?).ok()?;
    let mut shell = StoredObject::new(envelope, "", Value::Null);
    if let Some(index) = image.get("index") {
        shell.index = Deserialize::deserialize(index).ok()?;
    }
    blank_object(&mut shell);
    image["envelope"] = serde_json::to_value(&shell.envelope).ok()?;
    image["index"] = serde_json::to_value(&shell.index).ok()?;
    image.as_object_mut()?.remove("original");
    Some(())
}

/// What a reader learns of an object marked later: that it is gone. Not that it was marked.
pub fn plain_delete_envelope(id: ObjectId) -> Envelope {
    let mut e = sensitive_stub_envelope(id);
    e.sensitive.clear();
    e
}

/// All a sensitive object says of itself: its id and that it is sensitive. No type, name, time,
/// audience or category (ADR 0029). The hub needs nothing else to withhold it.
pub fn sensitive_stub_envelope(id: ObjectId) -> Envelope {
    let mut e = Envelope::new(STUB_TYPE, STUB_NAME);
    e.id = id;
    e.published = DateTime::<Utc>::UNIX_EPOCH;
    e.updated = DateTime::<Utc>::UNIX_EPOCH;
    e.sensitive = vec![SENSITIVE_MARKER.to_string()];
    e
}

/// [`sensitive_stub_envelope`] as an image, without content.
pub fn sensitive_stub(id: ObjectId) -> StoredObject {
    StoredObject::new(sensitive_stub_envelope(id), "", Value::Null)
}

#[cfg(test)]
mod tests;
