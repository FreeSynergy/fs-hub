//! A redacted log entry is told apart from a deletion, which really has `after: null`.

use fs_hub_proto::is_redacted;
use fs_objects::{Activity, ActivityId, ActivityKind, ObjectId};

fn entry(kind: ActivityKind, payload: serde_json::Value) -> Activity {
    Activity {
        id: ActivityId::new(),
        kind,
        object: ObjectId::new(),
        actor: "a".into(),
        at: chrono::Utc::now(),
        target: None,
        payload,
        synced: false,
    }
}

#[test]
fn the_fixed_payload_of_fs_objects_is_redacted() {
    let p = serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"});
    for kind in [
        ActivityKind::Create,
        ActivityKind::Update,
        ActivityKind::Delete,
    ] {
        assert!(is_redacted(&entry(kind, p.clone())), "{kind:?}");
    }
}

#[test]
fn a_real_delete_is_not_redacted() {
    let gone = serde_json::json!({"before": {"envelope": {}}, "after": null});
    assert!(!is_redacted(&entry(ActivityKind::Delete, gone)));
    assert!(!is_redacted(&entry(
        ActivityKind::Delete,
        serde_json::Value::Null
    )));
    let no = serde_json::json!({"redacted": false, "before": null, "after": null});
    assert!(!is_redacted(&entry(ActivityKind::Update, no)));
    let text = serde_json::json!({"redacted": "true"});
    assert!(!is_redacted(&entry(ActivityKind::Update, text)));
}

#[test]
fn the_store_refuses_to_read_images_from_it() {
    let p = serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"});
    let err = entry(ActivityKind::Update, p).change_set().unwrap_err();
    assert!(matches!(err, fs_objects::Error::Invalid(_)), "{err:?}");
}
