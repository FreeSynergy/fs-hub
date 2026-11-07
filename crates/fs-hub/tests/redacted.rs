//! End-to-end: an entry without state (purged history) deletes nothing at the hub, is not told as a
//! deletion, and a real delete still removes the copy. Sent raw, since fs-store 0.2 cannot purge.

mod common;

use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_hub_client::ChangeNotice;
use fs_store::traits::ObjectRepository;
use fs_store::{Activity, ActivityId, ActivityKind};
use std::time::Duration;

fn redacted_entry(kind: ActivityKind, object: fs_store::ObjectId) -> Activity {
    Activity {
        id: ActivityId::new(),
        kind,
        object,
        actor: "ada".into(),
        at: chrono::Utc::now(),
        target: None,
        payload: serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"}),
        synced: false,
    }
}

async fn next(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ChangeNotice>,
    ms: u64,
) -> Option<ChangeNotice> {
    tokio::time::timeout(Duration::from_millis(ms), rx.recv())
        .await
        .ok()
        .flatten()
}

#[tokio::test]
async fn an_entry_without_state_deletes_nothing_and_a_delete_does() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
            let owner = session(&env, notes_manifest("owner")).await;
            let reader = session(&env, notes_manifest("reader")).await;
            let mut rx = reader.subscribe().await.unwrap();
            let put = owner_store.put(note("kept", 8), "ada").await.unwrap();
            owner.sync_once(owner_store.as_ref()).await.unwrap();
            reader.sync_once(reader_store.as_ref()).await.unwrap();
            assert!(reader_store.get(put.object).await.unwrap().is_some());
            assert!(matches!(
                next(&mut rx, 5000).await,
                Some(ChangeNotice::Created { .. })
            ));

            // Purged history, as another program would push it: skipped, not a problem, not a delete.
            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let entries: Vec<_> = [ActivityKind::Delete, ActivityKind::Update]
                .into_iter()
                .map(|k| redacted_entry(k, put.object))
                .collect();
            let outcome = raw.push(&entries, "ada").await;
            assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
            assert_eq!((outcome.accepted, outcome.skipped), (0, 2));
            let report = reader.sync_once(reader_store.as_ref()).await.unwrap();
            assert_eq!(report.pulled, 0, "nothing for a reader");
            assert!(
                reader_store.get(put.object).await.unwrap().is_some(),
                "the copy stays"
            );
            assert!(next(&mut rx, 500).await.is_none(), "not told as deleted");
            let late = env.store("late").await;
            session(&env, notes_manifest("late"))
                .await
                .sync_once(late.as_ref())
                .await
                .unwrap();
            assert!(
                late.get(put.object).await.unwrap().is_some(),
                "the hub still holds it"
            );

            // A real delete removes the copy.
            owner_store.delete(put.object, "ada").await.unwrap();
            owner.sync_once(owner_store.as_ref()).await.unwrap();
            reader.sync_once(reader_store.as_ref()).await.unwrap();
            assert!(reader_store.get(put.object).await.unwrap().is_none());
            assert!(matches!(
                next(&mut rx, 5000).await,
                Some(ChangeNotice::Deleted { .. })
            ));
        })
        .await;
}
