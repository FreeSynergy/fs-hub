//! End-to-end: what came from the hub never goes back, so an old version cannot roll it back.

mod common;

use common::{Env, note, notes_manifest, session};
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{ActivityFilter, StoredObject};

fn with_text(mut o: StoredObject, text: &str) -> StoredObject {
    o.content["text"] = serde_json::json!(text);
    o
}

/// The state a crash leaves between applying and marking: `hub:*` entries of an old version, unsynced.
#[tokio::test]
async fn a_crash_between_apply_and_mark_rolls_nothing_back() {
    let env = Env::start();
    let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let o = with_text(note("n", 8), "v1");
    let id = o.envelope.id;
    reader_store.put(o.clone(), "hub:owner").await.unwrap();

    owner_store.put(o.clone(), "ada").await.unwrap();
    owner_store.put(with_text(o, "v2"), "ada").await.unwrap();
    owner.sync_once(owner_store.as_ref()).await.unwrap();

    let report = reader.sync_once(reader_store.as_ref()).await.unwrap();
    assert_eq!(report.pushed, 0, "it does not go");
    let hub = env.hub().core().store();
    assert_eq!(hub.get(id).await.unwrap().unwrap().content["text"], "v2");
    assert_eq!(
        reader_store.get(id).await.unwrap().unwrap().content["text"],
        "v2"
    );
    owner.sync_once(owner_store.as_ref()).await.unwrap();
    assert_eq!(
        owner_store.get(id).await.unwrap().unwrap().content["text"],
        "v2"
    );

    reader.sync_once(reader_store.as_ref()).await.unwrap();
    let unsynced = ActivityFilter {
        unsynced_only: true,
        ..Default::default()
    };
    assert_eq!(reader_store.activities(&unsynced).await.unwrap().len(), 0);
}

/// No crash needed: a `hub:*` entry behind the program's own unsynced entry.
#[tokio::test]
async fn a_hub_entry_behind_an_own_entry_stays_home_and_the_own_one_goes() {
    let env = Env::start();
    let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;

    let own = note("own", 8);
    let own_id = own.envelope.id;
    reader_store.put(own, "me").await.unwrap();
    let o = with_text(note("n", 8), "v1");
    let id = o.envelope.id;
    reader_store.put(o.clone(), "hub:owner").await.unwrap();
    owner_store.put(with_text(o, "v2"), "ada").await.unwrap();
    owner.sync_once(owner_store.as_ref()).await.unwrap();

    let report = reader.sync_once(reader_store.as_ref()).await.unwrap();
    assert_eq!((report.pushed, report.accepted), (1, 1));
    let hub = env.hub().core().store();
    assert!(hub.get(own_id).await.unwrap().is_some());
    assert_eq!(hub.get(id).await.unwrap().unwrap().content["text"], "v2");
}
