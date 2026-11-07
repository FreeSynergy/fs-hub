//! A mark can be lifted after `purge_object`, by the owner only: its next push has no `before` image
//! (the history is gone), so the hub decides by the state of `after` and by who pushes.
//!
//! Pattern: Scenario test — one story per test, told through the real servers, the hub and the client.

mod common;

use common::story::Story;
use common::{held, lift_mark, mark_sensitive, people};
use fs_objects::{Activity, ActivityId, ActivityKind};

#[tokio::test]
async fn the_owner_lifts_a_mark_after_a_purge() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            s.settle().await;
            assert!(
                held(&s.c.store, s.ada).await.is_none(),
                "the mark took it off"
            );
            s.purge().await;
            lift_mark(&s.p.store, s.ada, "me").await;

            let report = s.p.session.sync_once(s.p.store.as_ref()).await.unwrap();
            assert_eq!(
                (report.pushed, report.accepted, report.withheld),
                (1, 1, 0),
                "{report:?}"
            );
            s.settle().await;
            assert!(
                held(&s.c.store, s.ada).await.is_some(),
                "the copy is back at the reader"
            );
            let again = s.p.session.sync_once(s.p.store.as_ref()).await.unwrap();
            assert_eq!(again.pushed, 0, "nothing stays unsynced");
        })
        .await;
}

#[tokio::test]
async fn a_stranger_cannot_lift_the_mark() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            s.settle().await;
            s.purge().await;
            // Another program brings an unmarked copy of the card without a `before` image.
            let stranger = s.env.session(people::manifest("contacts")).await;
            let mut copy = held(&s.p.store, s.ada).await.unwrap();
            copy.envelope.sensitive.clear();
            let entry = Activity {
                id: ActivityId::new(),
                kind: ActivityKind::Update,
                object: copy.envelope.id,
                actor: String::new(),
                at: chrono::Utc::now(),
                target: None,
                payload: serde_json::json!({"before": null, "after": copy}),
                synced: false,
            };
            let outcome = stranger.push(vec![entry]).await.unwrap();
            assert_eq!(outcome.accepted, 0, "{outcome:?}");
            s.settle().await;
            assert!(
                held(&s.c.store, s.ada).await.is_none(),
                "the mark still holds"
            );

            // The owner still can.
            lift_mark(&s.p.store, s.ada, "me").await;
            s.settle().await;
            assert!(held(&s.c.store, s.ada).await.is_some());
        })
        .await;
}
