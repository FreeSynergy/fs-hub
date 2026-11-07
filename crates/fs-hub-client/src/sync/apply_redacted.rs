//! A pulled entry without state deletes nothing here (test of `apply`).
//!
//! Pattern: none (tests of `apply` for entries without state).

use fs_store::traits::ObjectRepository;
use fs_store::{Activity, ActivityId, ActivityKind, Envelope, StoredObject, TursoStore};

use super::apply::apply;

#[tokio::test]
async fn a_redacted_entry_deletes_nothing() {
    let store = TursoStore::open_in_memory().await.unwrap();
    let kept = store
        .put(
            StoredObject::new(
                Envelope::new("Note", "n"),
                "application/json",
                serde_json::json!({}),
            ),
            "me",
        )
        .await
        .unwrap();
    for kind in [
        ActivityKind::Delete,
        ActivityKind::Update,
        ActivityKind::Create,
    ] {
        let a = Activity {
            id: ActivityId::new(),
            kind,
            object: kept.object,
            actor: "other".into(),
            at: chrono::Utc::now(),
            target: None,
            payload: serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"}),
            synced: false,
        };
        assert!(!apply(&store, a).await.unwrap(), "{kind:?} changed nothing");
        assert!(store.get(kept.object).await.unwrap().is_some(), "{kind:?}");
    }
}

#[tokio::test]
async fn a_real_delete_still_deletes() {
    let store = TursoStore::open_in_memory().await.unwrap();
    let put = store
        .put(
            StoredObject::new(
                Envelope::new("Note", "n"),
                "application/json",
                serde_json::json!({}),
            ),
            "me",
        )
        .await
        .unwrap();
    let a = Activity {
        id: ActivityId::new(),
        kind: ActivityKind::Delete,
        object: put.object,
        actor: "other".into(),
        at: chrono::Utc::now(),
        target: None,
        payload: serde_json::json!({"before": null, "after": null}),
        synced: false,
    };
    assert!(apply(&store, a).await.unwrap());
    assert!(store.get(put.object).await.unwrap().is_none());
}
