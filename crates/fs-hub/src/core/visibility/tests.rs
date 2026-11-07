//! A redacted entry is visible to nobody: its type is unknown.

use fs_store::{ActivityId, ActivityKind, ObjectId, TursoStore};

use super::*;

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
