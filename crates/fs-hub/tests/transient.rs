//! A failure that may pass moves no cursor; a bad entry does. Real stores, with their fault hook.

mod common;

use std::sync::Arc;

use common::{Env, note, notes_manifest, session};
use fs_hub::{
    ActivitySink, MarkedRemovals, ObjectCollector, OriginalTimes, ProgramRegistry, StoreCursors,
    StoreMarked, StoreOriginals, StoredRegistry,
};
use fs_hub_proto::Pushed;
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{ActivityFilter, ActivityKind, TursoStore};

#[tokio::test]
async fn at_the_reader_it_stops_without_ack_and_the_entry_comes_again() {
    let env = Env::start();
    let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    owner_store.put(o, "ada").await.unwrap();
    owner.sync_once(owner_store.as_ref()).await.unwrap();

    reader_store.inject_fault_before_activity(true);
    let failed = reader.sync_once(reader_store.as_ref()).await;
    assert!(failed.is_err(), "not reported as a bad entry: {failed:?}");
    assert_eq!(
        reader.pull(None, 0).await.unwrap().activities.len(),
        1,
        "not acked"
    );

    let report = reader.sync_once(reader_store.as_ref()).await.unwrap();
    assert_eq!((report.applied, report.failed.len()), (1, 0));
    assert!(reader_store.get(id).await.unwrap().is_some());
}

#[tokio::test]
async fn at_the_hub_it_stops_at_the_entry_and_a_bad_entry_is_passed() {
    let dir = tempfile::tempdir().unwrap();
    let open = |name: &str| TursoStore::open(dir.path().join(name));
    let objects = Arc::new(open("objects.db").await.unwrap());
    let state = Arc::new(open("state.db").await.unwrap());
    let registry: Arc<dyn ProgramRegistry> = Arc::new(
        StoredRegistry::open(Arc::new(StoreCursors::new(state.clone()).await.unwrap()))
            .await
            .unwrap(),
    );
    let _live = registry.register(notes_manifest("wiki")).await.unwrap();
    let originals: Arc<dyn OriginalTimes> = Arc::new(StoreOriginals::new(state.clone()));
    let marked: Arc<dyn MarkedRemovals> = Arc::new(StoreMarked::new(state));
    let sink = ObjectCollector::new(
        registry,
        objects.clone(),
        objects.clone(),
        originals,
        marked,
    );

    let source = open("source.db").await.unwrap();
    for name in ["first", "second", "third"] {
        source.put(note(name, 8), "wiki").await.unwrap();
    }
    let log = source.activities(&ActivityFilter::default()).await.unwrap();
    let pushed = |from: usize| -> Vec<Pushed> {
        log[from..]
            .iter()
            .map(|a| Pushed {
                activity: a.clone(),
                original_at: None,
            })
            .collect()
    };

    // A bad entry: reported, passed.
    let mut bad = pushed(0);
    bad[0].activity.payload = serde_json::json!("not a change set");
    assert_eq!(bad[0].activity.kind, ActivityKind::Create);
    let outcome = sink.push("wiki", vec![bad.remove(0)]).await.unwrap();
    assert_eq!(outcome.problems.len(), 1);
    assert_eq!(outcome.last_accepted, Some(log[0].id));

    // A failure that may pass: no problem entry and no move past it.
    objects.inject_fault_before_activity(true);
    let failed = sink.push("wiki", pushed(1)).await;
    assert!(failed.is_err(), "{failed:?}");
    let again = sink.push("wiki", pushed(1)).await.unwrap();
    assert_eq!(
        (again.accepted, again.skipped),
        (2, 0),
        "{:?}",
        again.problems
    );
    assert_eq!(again.last_accepted, Some(log[2].id));
}
