//! End-to-end: answers are cut by size, not only by count (80 notes of 1 MiB killed the connection).

mod common;

use common::{Env, note, notes_manifest, session};
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{ActivityFilter, ActivityId, ObjectId};

const NOTES: usize = 80;
const MIB: usize = 1 << 20;

/// Fill the hub with `NOTES` notes, pushed in pieces so only the way back is large.
async fn fill(env: &Env) -> Vec<ObjectId> {
    let store = env.store("wiki").await;
    let wiki = session(env, notes_manifest("wiki")).await;
    let mut ids = Vec::new();
    for round in 0..NOTES / 8 {
        for n in 0..8 {
            let o = note(&format!("Note {}", round * 8 + n), MIB);
            ids.push(o.envelope.id);
            store.put(o, "wiki").await.unwrap();
        }
        wiki.sync_once(store.as_ref()).await.unwrap();
    }
    ids
}

#[tokio::test]
async fn eighty_large_notes_arrive_once_and_in_order_across_pages() {
    let env = Env::start();
    let expected = fill(&env).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let (mut seen, mut pages, mut last_seen): (Vec<ObjectId>, usize, Option<ActivityId>) =
        (Vec::new(), 0, None);
    loop {
        let page = reader.pull(None, 0).await.expect("the default batch works");
        pages += 1;
        assert!(pages <= NOTES, "paging must advance");
        for a in &page.activities {
            assert!(last_seen.is_none_or(|l| a.id > l), "ids only go up");
            last_seen = Some(a.id);
            seen.push(a.object);
        }
        if !page.more {
            reader.ack(page.last.unwrap()).await.unwrap();
            break;
        }
        assert!(
            !page.activities.is_empty(),
            "a page always holds an activity"
        );
        assert_eq!(page.last, last_seen, "`last` is the last delivered");
        reader.ack(page.last.unwrap()).await.unwrap();
    }
    assert_eq!(seen, expected, "each exactly once, in order");
    assert!(pages > 1, "80 MiB cannot be one page");
    assert!(
        reader.programs().await.is_ok(),
        "the connection still works"
    );
    assert!(reader.pull(None, 0).await.unwrap().activities.is_empty());
}

#[tokio::test]
async fn a_small_limit_still_wins_and_an_unacked_page_repeats() {
    let env = Env::start();
    fill(&env).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let first = reader.pull(None, 3).await.unwrap();
    assert_eq!(first.activities.len(), 3);
    assert!(first.more);
    let again = reader.pull(None, 3).await.unwrap();
    assert_eq!(
        again.last, first.last,
        "pull alone does not move the cursor"
    );
    let after = reader.pull(first.last, 3).await.unwrap();
    assert!(after.activities[0].id > first.last.unwrap());
}

#[tokio::test]
async fn sync_once_carries_large_objects_both_ways() {
    let env = Env::start();
    let (a_store, b_store) = (env.store("a").await, env.store("b").await);
    let a = session(&env, notes_manifest("a")).await;
    let b = session(&env, notes_manifest("b")).await;
    let mut ids = Vec::new();
    for i in 0..NOTES {
        let o = note(&format!("A{i}"), MIB);
        ids.push(o.envelope.id);
        a_store.put(o, "a").await.unwrap();
    }
    let up = a.sync_once(a_store.as_ref()).await.expect("large push");
    assert_eq!((up.pushed, up.accepted as usize), (NOTES, NOTES));
    let down = b.sync_once(b_store.as_ref()).await.expect("large pull");
    assert_eq!((down.pulled, down.applied), (NOTES, NOTES));
    assert!(down.failed.is_empty(), "{:?}", down.failed);
    let got = b_store.get(ids[NOTES - 1]).await.unwrap().unwrap();
    assert_eq!(got.content["text"].as_str().unwrap().len(), MIB);

    // And back: every object updated, so each activity carries a before and an after image.
    for id in &ids {
        let mut o = b_store.get(*id).await.unwrap().unwrap();
        o.content["text"] = serde_json::json!("y".repeat(MIB));
        b_store.put(o, "b").await.unwrap();
    }
    let up = b
        .sync_once(b_store.as_ref())
        .await
        .expect("large push back");
    assert_eq!(up.accepted as usize, NOTES);
    let down = a
        .sync_once(a_store.as_ref())
        .await
        .expect("large pull back");
    assert_eq!((down.pulled, down.applied), (NOTES, NOTES));
    let got = a_store.get(ids[0]).await.unwrap().unwrap();
    assert!(got.content["text"].as_str().unwrap().starts_with('y'));

    // Nothing travels back, and both connections still work.
    assert_eq!(a.sync_once(a_store.as_ref()).await.unwrap().pushed, 0);
    assert_eq!(b.sync_once(b_store.as_ref()).await.unwrap().pulled, 0);
    assert_eq!(a.programs().await.unwrap().len(), 2);
    let unsynced = ActivityFilter {
        unsynced_only: true,
        ..Default::default()
    };
    assert!(a_store.activities(&unsynced).await.unwrap().is_empty());
}
