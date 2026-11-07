//! A redacted entry is visible to nobody: its type is unknown.
//!
//! Pattern: none (tests of `Visibility`).

use std::sync::Arc;

use fs_objects::{Activity, ActivityId, ActivityKind, ObjectId, TursoStore};

use super::visibility::Visibility;

#[tokio::test]
async fn a_redacted_entry_is_visible_to_nobody() {
    let store = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let rule = Visibility::new(store);
    let payload = serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"});
    for kind in [
        ActivityKind::Create,
        ActivityKind::Update,
        ActivityKind::Delete,
    ] {
        let a = Activity {
            id: ActivityId::new(),
            kind,
            object: ObjectId::new(),
            actor: "owner".into(),
            at: chrono::Utc::now(),
            target: None,
            payload: payload.clone(),
            synced: false,
        };
        let reads = vec!["Note".to_string()];
        assert!(
            !rule.allows("reader", &reads, &a).await.unwrap(),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn a_relationship_is_visible_to_nobody_whatever_the_manifest_says() {
    let store = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let rule = Visibility::new(store);
    let object = fs_objects::StoredObject::new(
        fs_objects::Envelope::new("Relationship", "r"),
        "application/json",
        serde_json::json!({}),
    );
    let cs = fs_objects::ChangeSet {
        before: None,
        after: Some(object),
    };
    let a = Activity {
        id: ActivityId::new(),
        kind: ActivityKind::Create,
        object: ObjectId::new(),
        actor: "people".into(),
        at: chrono::Utc::now(),
        target: None,
        payload: serde_json::to_value(cs).unwrap(),
        synced: false,
    };
    let reads = vec!["Relationship".to_string(), "Profile".to_string()];
    assert!(!rule.allows("greedy", &reads, &a).await.unwrap());
}
