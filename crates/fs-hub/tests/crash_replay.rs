//! The real binary, killed right after answering a push: what it acknowledged stays acknowledged, so a
//! program that lost its own mark and pushes again cannot roll back another program's newer edit.

mod common;

use common::binary::{connect, kill, start};
use common::note;
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{ActivityFilter, TursoStore};

#[tokio::test]
async fn a_replay_after_a_crash_overwrites_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (socket, data) = (dir.path().join("hub.sock"), dir.path().join("hub-data"));
    let hub = start(&socket, &data);
    let (a, b) = (connect(&socket, "a").await, connect(&socket, "b").await);
    let a_store = TursoStore::open(dir.path().join("a.db")).await.unwrap();
    let b_store = TursoStore::open(dir.path().join("b.db")).await.unwrap();

    // Well inside a second: a's second push, then b's newer edit, then the crash.
    a_store.put(note("first", 8), "ada").await.unwrap();
    let log = a_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    a.push(log).await.unwrap();
    let o = note("n", 8);
    let id = o.envelope.id;
    a_store.put(o.clone(), "ada").await.unwrap();
    let log = a_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    a.push(log.clone()).await.unwrap();
    let mut newer = o;
    newer.content["text"] = serde_json::json!("b's newer edit");
    b_store.put(newer, "bob").await.unwrap();
    b.sync_once(&b_store).await.unwrap();
    drop((a, b));
    kill(hub);

    let hub = start(&socket, &data);
    let a = connect(&socket, "a").await;
    let replay = a.push(log).await.unwrap();
    assert_eq!((replay.accepted, replay.skipped), (0, 2), "{replay:?}");
    let reader = connect(&socket, "reader").await;
    let page = reader.pull(None, 0).await.unwrap();
    let last = page.activities.iter().rfind(|e| e.object == id).unwrap();
    let after = last.change_set().unwrap().after.unwrap();
    assert_eq!(after.content["text"], "b's newer edit");
    kill(hub);
}
