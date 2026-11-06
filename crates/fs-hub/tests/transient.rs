//! A failure that may pass moves no cursor; a bad entry does. Real stores, with their fault hook.

mod common;

use common::parts::{Parts, pushed};
use common::{Env, note, notes_manifest, session};
use fs_hub::ActivitySink;
use fs_hub_client::Error;
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

/// An entry that changes nothing, then one that fails: the first is acknowledged, the second comes again.
#[tokio::test]
async fn what_was_applied_is_acknowledged_before_it_stops() {
    let env = Env::start();
    let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let same = note("same", 8);
    reader_store.put(same.clone(), "hub:owner").await.unwrap();
    owner_store.put(same, "ada").await.unwrap();
    owner_store.put(note("next", 8), "ada").await.unwrap();
    owner.sync_once(owner_store.as_ref()).await.unwrap();

    reader_store.inject_fault_before_activity(true);
    let stopped = reader.sync_once(reader_store.as_ref()).await.unwrap_err();
    let Error::Stopped { report, .. } = stopped else {
        panic!("{stopped}");
    };
    assert_eq!(report.pulled, 2, "what it learned comes with the error");
    let rest = reader.pull(None, 0).await.unwrap();
    assert_eq!(rest.activities.len(), 1, "only what failed comes again");
}

#[tokio::test]
async fn at_the_hub_it_stops_at_the_entry_and_a_bad_entry_is_passed() {
    let dir = tempfile::tempdir().unwrap();
    let parts = Parts::open(dir.path()).await;
    let sink = &parts.collector;

    let source = TursoStore::open(dir.path().join("source.db"))
        .await
        .unwrap();
    for name in ["first", "second", "third"] {
        source.put(note(name, 8), "owner").await.unwrap();
    }
    let log = source.activities(&ActivityFilter::default()).await.unwrap();

    // A bad entry: reported, passed.
    let mut bad = pushed(&log[..1]);
    bad[0].activity.payload = serde_json::json!("not a change set");
    assert_eq!(bad[0].activity.kind, ActivityKind::Create);
    let outcome = sink.push("owner", bad).await.unwrap();
    assert_eq!(outcome.problems.len(), 1);
    assert_eq!(outcome.last_accepted, Some(log[0].id));

    // A failure that may pass: no problem entry and no move past it.
    parts.objects.inject_fault_before_activity(true);
    let failed = sink.push("owner", pushed(&log[1..])).await;
    assert!(failed.is_err(), "{failed:?}");
    let again = sink.push("owner", pushed(&log[1..])).await.unwrap();
    assert_eq!(
        (again.accepted, again.skipped),
        (2, 0),
        "{:?}",
        again.problems
    );
    assert_eq!(again.last_accepted, Some(log[2].id));
}
