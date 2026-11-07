//! What may travel of a log entry.
//!
//! Pattern: Specification checked by example.

use fs_hub_proto::MAX_HUB_MESSAGE_BYTES;
use fs_objects::{ActivityId, ActivityKind, ChangeSet, StoredObject};

use super::*;

fn image(bytes: usize, sensitive: bool) -> StoredObject {
    let mut e = Envelope::new("Note", "n");
    if sensitive {
        e.sensitive = vec!["pd:Health".into()];
    }
    let content = serde_json::json!({ "text": "x".repeat(bytes) });
    StoredObject::new(e, "application/json", content)
}

fn change(before: Option<StoredObject>, after: Option<StoredObject>) -> Activity {
    Activity {
        id: ActivityId::new(),
        kind: ActivityKind::Update,
        object: ObjectId::new(),
        actor: "person".into(),
        at: chrono::Utc::now(),
        target: None,
        payload: serde_json::to_value(ChangeSet { before, after }).unwrap(),
        synced: false,
    }
}

const LARGE: usize = MAX_OBJECT_BYTES + 1;

#[test]
fn pieces_keep_the_order_and_stay_below_a_page() {
    let log: Vec<_> = (0..80)
        .map(|_| change(None, Some(image(1 << 20, false))))
        .collect();
    let ids: Vec<_> = log.iter().map(|a| a.id).collect();
    let plan = plan(log);
    assert!(plan.pieces.len() > 1);
    let sent: Vec<_> = plan.pieces.iter().flatten().map(|a| a.id).collect();
    assert_eq!(sent, ids);
    for piece in &plan.pieces {
        let bytes: usize = piece.iter().map(|a| json_len(&a.payload)).sum();
        assert!(bytes < MAX_HUB_MESSAGE_BYTES / 2, "{bytes}");
    }
}

#[test]
fn a_large_object_stays_here_and_the_log_moves_past_it() {
    let large = change(None, Some(image(LARGE, false)));
    let id = large.id;
    let plan = plan(vec![large]);
    assert_eq!(plan.pieces.len(), 0);
    assert_eq!(plan.refused.last_accepted, Some(id));
    assert_eq!(plan.refused.problems[0].status, 413);
    assert!(plan.refused.problems[0].detail.contains(&id.to_string()));
}

#[test]
fn a_large_before_image_does_not_travel() {
    let plan = plan(vec![change(
        Some(image(LARGE, false)),
        Some(image(8, false)),
    )]);
    let cs = plan.pieces[0][0].change_set().unwrap();
    assert!(cs.before.is_none() && cs.after.is_some());
    assert_eq!(plan.refused.problems.len(), 0);
}

#[test]
fn unsorted_input_is_sent_in_log_order() {
    let (first, second) = (
        change(None, Some(image(8, false))),
        change(None, Some(image(8, false))),
    );
    let ids = [first.id, second.id];
    let plan = plan(vec![second, first]);
    let sent: Vec<_> = plan.pieces[0].iter().map(|a| a.id).collect();
    assert_eq!(sent, ids);
}

#[test]
fn a_sensitive_image_says_only_that_whatever_its_size() {
    for bytes in [8, LARGE] {
        let plan = plan(vec![change(
            Some(image(bytes, true)),
            Some(image(bytes, true)),
        )]);
        assert_eq!(plan.refused.problems.len(), 0, "the hub has to hear of it");
        let sent = &plan.pieces[0][0];
        assert_eq!(sent.at, DateTime::<Utc>::UNIX_EPOCH);
        let cs = sent.change_set().unwrap();
        let after = cs.after.unwrap();
        assert!(after.content.is_null() && cs.before.unwrap().content.is_null());
        assert_eq!(after.envelope.sensitive, [fs_hub_proto::SENSITIVE_MARKER]);
        assert_eq!(after.envelope.id, sent.object);
        assert_ne!(after.envelope.name, "n", "a neutral name");
        assert_ne!(after.envelope.object_type, "Note", "a neutral type");
        assert_eq!(after.envelope.published, DateTime::<Utc>::UNIX_EPOCH);
    }
}

#[test]
fn taking_a_mark_off_keeps_the_object() {
    let plan = plan(vec![change(Some(image(8, true)), Some(image(8, false)))]);
    let cs = plan.pieces[0][0].change_set().unwrap();
    assert!(cs.before.unwrap().content.is_null(), "was sensitive");
    let after = cs.after.unwrap();
    assert_eq!(after.envelope.name, "n");
    assert_eq!(after.content["text"], "x".repeat(8));
}

#[test]
fn setting_a_mark_keeps_nothing_of_the_plain_image_out_of_place() {
    let plan = plan(vec![change(Some(image(8, false)), Some(image(8, true)))]);
    let cs = plan.pieces[0][0].change_set().unwrap();
    assert!(cs.after.unwrap().content.is_null());
}

#[test]
fn a_large_plain_after_is_refused_though_the_before_is_sensitive() {
    let plan = plan(vec![change(
        Some(image(8, true)),
        Some(image(LARGE, false)),
    )]);
    assert_eq!(plan.refused.problems.len(), 1);
    assert_eq!(plan.pieces.len(), 0);
}
