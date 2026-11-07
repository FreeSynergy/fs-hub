//! A redacted entry is never told, least of all as "deleted".

use fs_store::{ActivityId, ObjectId};

use super::*;

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
    let gone = serde_json::json!({"before": {"envelope": {"id": ObjectId::new(), "type": "Note", "name": "n"}}, "after": null});
    let notice = notice_of(&entry(ActivityKind::Delete, gone));
    assert!(matches!(notice, Ok(Some(Notice::Deleted(_)))), "{notice:?}");
}
