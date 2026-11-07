//! What a reader gets of an object.
//!
//! Pattern: Specification checked by example.

use fs_objects::Blob;

use super::*;

fn full() -> StoredObject {
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
    StoredObject::new(e, "text/plain", serde_json::json!({"text": 1}))
        .with_index("person", "https://example.org/ada")
        .with_index("kind", "note")
        .with_original(Blob {
            media_type: "message/rfc822".into(),
            bytes: b"Bcc: ada".to_vec(),
        })
}

#[test]
fn blanking_leaves_no_trace_and_keeping_brings_all_but_the_original_back() {
    let o = full();
    let mut blanked = o.clone();
    blank_object(&mut blanked);
    assert!(!format!("{blanked:?}").contains("ada"));
    assert_eq!(blanked.index["kind"], ["note"], "what a reader needs stays");
    keep_held_back(&o, &mut blanked);
    assert_eq!(blanked.envelope, o.envelope);
    assert_eq!(blanked.index, o.index);
}

#[test]
fn blanking_images_leaves_the_content_and_takes_the_rest() {
    let mut payload = serde_json::json!({"before": full(), "after": null});
    blank_images(&mut payload);
    assert!(!payload.to_string().contains("ada"), "{payload}");
    assert!(!payload.to_string().contains("QmNjOiBhZGE"), "the original");
    let back: fs_objects::ChangeSet = serde_json::from_value(payload).unwrap();
    let before = back.before.unwrap();
    assert_eq!(before.content["text"], 1);
    assert_eq!(before.index["kind"], ["note"]);
    assert!(back.after.is_none());
}

#[test]
fn what_cannot_be_read_does_not_go_out() {
    let mut payload = serde_json::json!({"after": {"envelope": 1, "text": "ada"}});
    blank_images(&mut payload);
    assert!(payload.is_null());
}

#[test]
fn a_plain_delete_has_no_marker() {
    assert_eq!(plain_delete_envelope(ObjectId::new()).sensitive.len(), 0);
}

#[test]
fn the_keys_of_a_card_go_only_with_a_card() {
    let with_keys = |object_type: &str| {
        let mut o = StoredObject::new(Envelope::new(object_type, "n"), "text/plain", Value::Null);
        for key in ["email", "phone", "nickname", "org", "subject"] {
            o = o.with_index(key, "x");
        }
        blank_object(&mut o);
        o.index.into_keys().collect::<Vec<_>>()
    };
    assert_eq!(with_keys("Profile"), ["email", "nickname", "org", "phone"]);
    assert_eq!(with_keys("Relationship"), ["subject"]);
    assert_eq!(with_keys("Event").len(), 0);
}
