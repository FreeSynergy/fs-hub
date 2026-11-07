//! End-to-end: two programs edit one object at once. The last push wins, everywhere.

mod common;

use common::{Env, note, notes_manifest, session};
use fs_objects::ActivityKind;
use fs_objects::traits::{ActivityLog, ObjectRepository};

#[tokio::test]
async fn two_concurrent_edits_end_the_same_everywhere() {
    let env = Env::start();
    let (a_store, b_store) = (env.store("a").await, env.store("b").await);
    let a = session(&env, notes_manifest("a")).await;
    let b = session(&env, notes_manifest("b")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    a_store.put(o, "ada").await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();

    for (store, text) in [(&a_store, "from a"), (&b_store, "from b")] {
        let mut copy = store.get(id).await.unwrap().unwrap();
        copy.content["text"] = serde_json::json!(text);
        store.put(copy, "someone").await.unwrap();
    }
    // a pushes first, then b: b's push is the last, and its pull must not take a's older one over it.
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();

    let hub = env.hub().core().store();
    for (name, text) in [
        ("hub", hub.get(id).await.unwrap().unwrap().content),
        ("a", a_store.get(id).await.unwrap().unwrap().content),
        ("b", b_store.get(id).await.unwrap().unwrap().content),
    ] {
        assert_eq!(text["text"], "from b", "{name}");
    }
    // A later reader sees all of it, in order.
    let late = session(&env, notes_manifest("late")).await;
    assert_eq!(late.pull(None, 0).await.unwrap().activities.len(), 3);
}

/// A log-only entry of the reader changes nothing: it must not swallow what another program wrote.
#[tokio::test]
async fn an_own_log_entry_does_not_swallow_an_update() {
    let env = Env::start();
    let (a_store, b_store) = (env.store("a").await, env.store("b").await);
    let a = session(&env, notes_manifest("a")).await;
    let b = session(&env, notes_manifest("b")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    a_store.put(o, "ada").await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();

    let mut copy = a_store.get(id).await.unwrap().unwrap();
    copy.content["text"] = serde_json::json!("from a");
    a_store.put(copy, "ada").await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b_store
        .log(ActivityKind::Read, id, "bob", serde_json::json!({}))
        .await
        .unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();
    let got = b_store.get(id).await.unwrap().unwrap();
    assert_eq!(got.content["text"], "from a");
}
