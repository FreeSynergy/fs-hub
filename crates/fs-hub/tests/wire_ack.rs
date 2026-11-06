//! End-to-end on the wire: an `ack` beyond what a session pulled with content is refused.

mod common;

use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_store::traits::ObjectRepository;

#[tokio::test]
async fn summaries_give_no_right_to_ack() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("owner").await;
            let owner = session(&env, notes_manifest("owner")).await;
            for i in 0..3 {
                store.put(note(&format!("N{i}"), 8), "ada").await.unwrap();
            }
            owner.sync_once(store.as_ref()).await.unwrap();

            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let summaries = raw.pull(None, 0, true).await;
            let last = summaries.last.unwrap();
            assert!(raw.ack(last).await.is_err(), "only summaries were read");
            assert_eq!(
                raw.pull(None, 0, true).await.activities.len(),
                3,
                "not moved"
            );

            // With content, up to where it got; not one further.
            let first = raw.pull(None, 1, false).await;
            let one = first.last.unwrap();
            assert!(raw.ack(last).await.is_err(), "beyond the content");
            raw.ack(one)
                .await
                .expect("what was pulled can be acknowledged");
            assert_eq!(raw.pull(None, 0, true).await.activities.len(), 2);
            assert!(!raw.pull(None, 0, false).await.more, "the connection stays");
        })
        .await;
}
