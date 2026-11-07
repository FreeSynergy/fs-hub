//! End-to-end: another program's mark does not block the owner. It goes on writing, the object stays away
//! from the hub, nothing is lost or counted as delivered, and when the mark lifts the owner's object wins.

mod common;

use common::{Env, config, note, notes_manifest, session};
use fs_hub::Daemon;
use fs_hub_client::MAX_OBJECT_BYTES;
use fs_objects::{ActivityFilter, ObjectId, Store, StoredObject};

const MARKERS_OLD: &str = "the marking program's old copy";

fn marked(mut o: StoredObject) -> StoredObject {
    o.envelope.sensitive = vec!["pd:Health".into()];
    o
}

fn with_text(mut o: StoredObject, text: &str) -> StoredObject {
    o.content["text"] = serde_json::json!(text);
    o
}

async fn unsynced(store: &dyn Store) -> usize {
    let filter = ActivityFilter {
        unsynced_only: true,
        ..Default::default()
    };
    store.activities(&filter).await.unwrap().len()
}

/// The owner wrote `id`; `other` holds a copy with its own text and marked it.
async fn marked_by_other(
    owner_store: &dyn Store,
    other_store: &dyn Store,
    owner: &fs_hub_client::HubSession,
    other: &fs_hub_client::HubSession,
) -> StoredObject {
    let o = note("n", 8);
    owner_store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(owner_store).await.unwrap();
    other.sync_once(other_store).await.unwrap();
    let copy = other_store.get(o.envelope.id).await.unwrap().unwrap();
    let copy = with_text(copy, MARKERS_OLD);
    other_store.put(marked(copy), "bob").await.unwrap();
    assert_eq!(other.sync_once(other_store).await.unwrap().withheld, 1);
    o
}

#[tokio::test]
async fn the_owner_goes_on_writing_and_its_newest_wins_when_the_mark_lifts() {
    let mut env = Env::start();
    let (store, other_store) = (env.store("owner").await, env.store("other").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let other = session(&env, notes_manifest("other")).await;
    let o = marked_by_other(store.as_ref(), other_store.as_ref(), &owner, &other).await;
    let id = o.envelope.id;
    let hub = env.hub().core().store();

    // Two writes of the held object, and between them a write of another one, which goes as usual.
    store.put(with_text(o.clone(), "v2"), "ada").await.unwrap();
    let elsewhere = note("elsewhere", 8);
    let elsewhere_id: ObjectId = elsewhere.envelope.id;
    store.put(elsewhere, "ada").await.unwrap();
    store.put(with_text(o.clone(), "v3"), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(up.problems.len(), 0, "not refused: {up:?}");
    assert_eq!((up.accepted, up.withheld), (1, 2), "{up:?}");
    assert!(hub.get(id).await.unwrap().is_none(), "it stays away");
    assert!(
        hub.get(elsewhere_id).await.unwrap().is_some(),
        "nothing else is blocked"
    );
    assert_eq!(
        unsynced(store.as_ref()).await,
        3,
        "not counted as delivered"
    );
    assert_eq!(store.get(id).await.unwrap().unwrap().content["text"], "v3");

    // That survives a restart, and nothing comes twice.
    drop((owner, other));
    env.hub.take().unwrap().stop().unwrap();
    env.hub = Some(Daemon::spawn(config(&env.dir)).unwrap());
    let hub = env.hub().core().store();
    let owner = session(&env, notes_manifest("owner")).await;
    let other = session(&env, notes_manifest("other")).await;
    let log_len = hub
        .activities(&ActivityFilter::default())
        .await
        .unwrap()
        .len();
    let again = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((again.accepted, again.problems.len()), (0, 0), "{again:?}");
    assert_eq!(
        hub.activities(&ActivityFilter::default())
            .await
            .unwrap()
            .len(),
        log_len
    );
    assert_eq!(unsynced(store.as_ref()).await, 3);

    // The marking program lifts it, with its old copy: that stays out.
    let copy = other_store.get(id).await.unwrap().unwrap();
    let mut open = copy;
    open.envelope.sensitive.clear();
    other_store.put(open, "bob").await.unwrap();
    let lift = other.sync_once(other_store.as_ref()).await.unwrap();
    assert_eq!((lift.accepted, lift.problems.len()), (0, 0), "{lift:?}");
    assert!(hub.get(id).await.unwrap().is_none());

    // The owner's next sync brings its newest, and all of it is delivered now.
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(up.accepted, 2, "{up:?}");
    assert_eq!(hub.get(id).await.unwrap().unwrap().content["text"], "v3");
    assert_eq!(unsynced(store.as_ref()).await, 0);
    let hub_log = hub.activities(&ActivityFilter::default()).await.unwrap();
    assert!(
        !format!("{hub_log:?}").contains(MARKERS_OLD),
        "never the old version"
    );

    other.sync_once(other_store.as_ref()).await.unwrap();
    assert_eq!(
        other_store.get(id).await.unwrap().unwrap().content["text"],
        "v3"
    );
    assert_eq!(
        owner.sync_once(store.as_ref()).await.unwrap().pushed,
        0,
        "it settles"
    );
}

/// An entry too large for the hub behind a held one must not take the held one along. It stays at home
/// alone in its batch, or goes in a batch with the held one: both are tried.
#[tokio::test]
async fn a_refused_entry_does_not_mark_past_a_held_one() {
    for small_before in [0, 3] {
        let env = Env::start();
        let (store, other_store) = (env.store("owner").await, env.store("other").await);
        let owner = session(&env, notes_manifest("owner")).await;
        let other = session(&env, notes_manifest("other")).await;
        let o = marked_by_other(store.as_ref(), other_store.as_ref(), &owner, &other).await;
        let id = o.envelope.id;
        // Small entries before it widen the read window, so that held and refused share a batch.
        for i in 0..small_before {
            store.put(note(&format!("S{i}"), 8), "ada").await.unwrap();
        }
        store.put(with_text(o, "v2"), "ada").await.unwrap();
        store
            .put(note("big", MAX_OBJECT_BYTES + 1), "ada")
            .await
            .unwrap();

        let up = owner.sync_once(store.as_ref()).await.unwrap();
        assert_eq!(up.problems.len(), 1, "{up:?}");
        assert_eq!(
            unsynced(store.as_ref()).await,
            2,
            "{small_before}: the held one stays"
        );

        let mut open = other_store.get(id).await.unwrap().unwrap();
        open.envelope.sensitive.clear();
        other_store.put(open, "bob").await.unwrap();
        other.sync_once(other_store.as_ref()).await.unwrap();
        owner.sync_once(store.as_ref()).await.unwrap();
        let hub = env.hub().core().store();
        assert_eq!(hub.get(id).await.unwrap().unwrap().content["text"], "v2");
        assert_eq!(unsynced(store.as_ref()).await, 0, "{small_before}");
    }
}
