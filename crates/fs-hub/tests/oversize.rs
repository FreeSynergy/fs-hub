//! End-to-end: an object above the limit is refused, and nobody gets stuck on it.

mod common;

use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_hub_client::{MAX_HUB_MESSAGE_BYTES, MAX_OBJECT_BYTES};
use fs_interfaces::rpc::TOO_LARGE;
use fs_store::ActivityFilter;
use fs_store::traits::{ActivityLog, ObjectRepository};

const LARGE: usize = MAX_OBJECT_BYTES + (1 << 20);
const ABOVE_A_DEFAULT_MESSAGE: usize = 70 << 20;

/// The hub's own check, for a program that does not use `fs-hub-client`.
#[tokio::test]
async fn the_hub_refuses_it_with_a_problem_and_the_cursor_moves_past() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("raw").await;
            // Above the 64 MiB a default reader takes: the hub reads it, and then refuses it.
            let big = note("big", ABOVE_A_DEFAULT_MESSAGE);
            let big_id = big.envelope.id;
            store.put(note("before", 8), "raw").await.unwrap();
            let refused = store.put(big, "raw").await.unwrap();
            store.put(note("after", 8), "raw").await.unwrap();
            let log = store.activities(&ActivityFilter::default()).await.unwrap();
            let last = log.last().unwrap().id;

            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let outcome = raw.push(&log, "").await;
            assert_eq!(outcome.accepted, 2);
            assert_eq!(outcome.last_accepted, Some(last));
            assert_eq!(outcome.problems.len(), 1);
            let problem = &outcome.problems[0];
            assert_eq!(
                (problem.problem_type.as_str(), problem.status),
                (TOO_LARGE, 413)
            );
            assert!(problem.detail.contains(&refused.id.to_string()));
            assert!(problem.detail.contains(&MAX_OBJECT_BYTES.to_string()));
            assert!(
                env.hub()
                    .core()
                    .store()
                    .get(big_id)
                    .await
                    .unwrap()
                    .is_none()
            );

            // Pushing it again changes nothing: the cursor is past it.
            let again = raw.push(&log, "").await;
            assert_eq!((again.accepted, again.skipped), (0, 3));
            assert!(again.problems.is_empty());
        })
        .await;
}

/// Larger than any message: without the client's check this would end the connection.
#[tokio::test]
async fn sync_once_reports_it_moves_on_and_the_reader_never_sees_it() {
    let env = Env::start();
    let (store, reader_store) = (env.store("wiki").await, env.store("reader").await);
    let wiki = session(&env, notes_manifest("wiki")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    store.put(note("before", 8), "wiki").await.unwrap();
    store
        .put(note("huge", MAX_HUB_MESSAGE_BYTES + (1 << 20)), "wiki")
        .await
        .unwrap();
    store.put(note("after", 8), "wiki").await.unwrap();

    let report = wiki
        .sync_once(store.as_ref())
        .await
        .expect("no dead session");
    assert_eq!(report.accepted, 2);
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].problem_type, TOO_LARGE);
    assert_eq!(
        wiki.sync_once(store.as_ref()).await.unwrap().pushed,
        0,
        "moved past"
    );
    assert_eq!(
        wiki.programs().await.unwrap().len(),
        2,
        "connection still works"
    );

    let down = reader.sync_once(reader_store.as_ref()).await.unwrap();
    assert_eq!((down.pulled, down.applied), (2, 2));
}

/// The largest allowed object, updated: two images in one activity, both ways.
#[tokio::test]
async fn the_largest_allowed_update_arrives() {
    let env = Env::start();
    let (a_store, b_store) = (env.store("a").await, env.store("b").await);
    let a = session(&env, notes_manifest("a")).await;
    let b = session(&env, notes_manifest("b")).await;
    let almost = MAX_OBJECT_BYTES - (1 << 16);
    let o = note("large", almost);
    let id = o.envelope.id;
    a_store.put(o.clone(), "a").await.unwrap();
    let mut changed = o;
    changed.content["text"] = serde_json::json!("y".repeat(almost));
    a_store.put(changed, "a").await.unwrap();

    let up = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!(
        (up.accepted, up.problems.len()),
        (2, 0),
        "{:?}",
        up.problems
    );
    let down = b.sync_once(b_store.as_ref()).await.unwrap();
    assert_eq!(
        (down.applied, down.failed.len()),
        (2, 0),
        "{:?}",
        down.failed
    );
    let got = b_store.get(id).await.unwrap().unwrap();
    assert!(got.content["text"].as_str().unwrap().starts_with('y'));
}

/// Made small again, the object travels, though its log entry still holds the large image.
#[tokio::test]
async fn an_object_made_small_again_arrives() {
    let env = Env::start();
    let store = env.store("wiki").await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    let large = note("large", LARGE);
    let id = large.envelope.id;
    let mut small = large.clone();
    store.put(large, "wiki").await.unwrap();
    small.content = serde_json::json!({ "text": "small" });
    store.put(small, "wiki").await.unwrap();

    let report = wiki.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((report.accepted, report.problems.len()), (1, 1));
    let held = env.hub().core().store().get(id).await.unwrap().unwrap();
    assert_eq!(held.content["text"], "small");
}
