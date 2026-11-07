//! A redacted entry is told what the object is now; nothing else changes.

use fs_store::{ActivityKind, Envelope, ObjectId, StoredObject};

use super::*;

fn entry(kind: ActivityKind, payload: serde_json::Value) -> Activity {
    Activity {
        id: fs_store::ActivityId::new(),
        kind,
        object: ObjectId::new(),
        actor: "a".into(),
        at: chrono::Utc::now(),
        target: None,
        payload,
        synced: false,
    }
}

fn redacted() -> serde_json::Value {
    serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"})
}

fn note() -> StoredObject {
    StoredObject::new(
        Envelope::new("Note", "n"),
        "application/json",
        serde_json::json!({}),
    )
}

#[test]
fn a_plain_entry_is_left_alone() {
    let a = entry(
        ActivityKind::Delete,
        serde_json::json!({"before": null, "after": null}),
    );
    let (id, payload) = (a.id, a.payload.clone());
    let out = with_current_copy(a, None);
    assert_eq!(
        (out.id, out.payload, out.kind),
        (id, payload, ActivityKind::Delete)
    );
}

#[test]
fn a_redacted_entry_becomes_the_current_copy() {
    let current = note();
    let a = entry(ActivityKind::Delete, redacted());
    let id = a.id;
    let out = with_current_copy(a, Some(&current));
    assert_eq!(out.id, id, "the hub counts ids");
    assert_eq!(
        out.kind,
        ActivityKind::Update,
        "not a delete: the object is there"
    );
    let cs: fs_store::ChangeSet = serde_json::from_value(out.payload).unwrap();
    assert_eq!(cs.after.unwrap().envelope.id, current.envelope.id);
}

#[test]
fn a_redacted_entry_of_a_gone_object_is_a_delete() {
    let out = with_current_copy(entry(ActivityKind::Update, redacted()), None);
    assert_eq!(out.kind, ActivityKind::Delete);
    let cs: fs_store::ChangeSet = serde_json::from_value(out.payload).unwrap();
    assert!(cs.after.is_none() && cs.before.is_none());
}
