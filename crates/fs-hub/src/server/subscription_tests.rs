//! A redacted entry is never told, least of all as "deleted".
//!
//! Pattern: none (tests of `notice_of`).

use fs_interfaces::rpc::Notice;
use fs_objects::{Activity, ActivityId, ActivityKind, ObjectId};

use super::subscription::notice_of;

fn entry(kind: ActivityKind, payload: serde_json::Value) -> Activity {
    Activity {
        id: ActivityId::new(),
        kind,
        object: ObjectId::new(),
        actor: "owner".into(),
        at: chrono::Utc::now(),
        target: None,
        payload,
        synced: false,
    }
}

#[test]
fn a_redacted_entry_tells_nothing() {
    let p = serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"});
    for kind in [
        ActivityKind::Create,
        ActivityKind::Update,
        ActivityKind::Delete,
    ] {
        assert!(
            matches!(notice_of(&entry(kind, p.clone())), Ok(None)),
            "{kind:?}"
        );
    }
}

#[test]
fn a_real_delete_is_still_told() {
    let object = fs_objects::StoredObject::new(
        fs_objects::Envelope::new("Note", "n"),
        "application/json",
        serde_json::json!({}),
    );
    let gone = serde_json::json!({"before": object, "after": null});
    let notice = notice_of(&entry(ActivityKind::Delete, gone));
    assert!(matches!(notice, Ok(Some(Notice::Deleted(_)))), "{notice:?}");
}
