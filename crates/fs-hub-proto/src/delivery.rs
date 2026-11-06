//! What a reader does not get of an object, and what a sensitive object says at all.
//!
//! PROVISIONAL (strategy session, 2026-11-06): the cores put the person's IRI into `attributedTo` and
//! `to`, so these fields would tell every reader who did it, past the empty `originalActor`. Until the
//! person can release them, they are blank at delivery. [`held_back`] is the one place that names
//! them, and [`blank_for_delivery`] the one place that blanks them: a later release per person and per
//! program decides here. The hub and the owner keep the values: a reader that pushes a blanked object
//! back must not overwrite them ([`keep_held_back`]), and it is not a change ([`crate::same_content`]).
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

/// The fields a reader does not get. The only list of them.
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

/// Blank what a reader does not get. Everything leaving the hub goes through this.
pub fn blank_for_delivery(e: &mut Envelope) {
    let (attributed_to, lists) = held_back(e);
    *attributed_to = None;
    for list in lists {
        list.clear();
    }
}

/// `into` came from a reader or from the hub: every held-back field it has blank takes the value in
/// `from`. So a blanked object going round overwrites nothing. A field its owner really emptied looks
/// the same and keeps the old value here: nobody gets it, and it cannot be told apart until the wire
/// says which fields were blanked.
pub fn keep_held_back(from: &Envelope, into: &mut Envelope) {
    let mut from = from.clone();
    let (from_attributed, from_lists) = held_back(&mut from);
    let (attributed, lists) = held_back(into);
    if attributed.is_none() {
        *attributed = from_attributed.take();
    }
    for (list, from_list) in lists.into_iter().zip(from_lists) {
        if list.is_empty() {
            std::mem::swap(list, from_list);
        }
    }
}

/// [`blank_for_delivery`] on the envelopes inside before/after images, in place: the content is not
/// touched and not copied. An envelope that cannot be read does not go out.
pub fn blank_images(payload: &mut Value) {
    for image in ["before", "after"] {
        let Some(slot) = payload.get_mut(image).and_then(|i| i.get_mut("envelope")) else {
            continue;
        };
        let blanked = Envelope::deserialize(&*slot).ok().and_then(|mut e| {
            blank_for_delivery(&mut e);
            serde_json::to_value(e).ok()
        });
        *slot = blanked.unwrap_or(Value::Null);
    }
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
mod tests {
    use super::*;

    fn full() -> Envelope {
        let mut e = Envelope::new("Note", "n");
        e.attributed_to = Some("https://example.org/ada".into());
        for list in [
            &mut e.to,
            &mut e.cc,
            &mut e.bto,
            &mut e.bcc,
            &mut e.audience,
        ] {
            list.push("https://example.org/ada".into());
        }
        e
    }

    #[test]
    fn blanking_leaves_no_trace_and_keeping_brings_all_back() {
        let e = full();
        let mut blanked = e.clone();
        blank_for_delivery(&mut blanked);
        assert!(!format!("{blanked:?}").contains("ada"));
        keep_held_back(&e, &mut blanked);
        assert_eq!(blanked, e);
    }

    #[test]
    fn blanking_images_keeps_the_content() {
        let object = StoredObject::new(full(), "text/plain", serde_json::json!({"ada": 1}));
        let mut payload = serde_json::json!({"before": object, "after": null});
        blank_images(&mut payload);
        let back: fs_store::ChangeSet = serde_json::from_value(payload).unwrap();
        let before = back.before.unwrap();
        assert_eq!(before.envelope.attributed_to, None);
        assert_eq!(before.envelope.to.len(), 0);
        assert_eq!(before.content["ada"], 1);
        assert!(back.after.is_none());
    }
}
