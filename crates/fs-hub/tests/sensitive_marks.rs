//! End-to-end through client and hub: marking an object sensitive, and taking the mark off.

mod common;

use common::{Env, config, note, notes_manifest, session};
use fs_hub::Daemon;
use fs_hub_client::MAX_OBJECT_BYTES;
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{ActivityFilter, ActivityKind, StoredObject};

const OLD: &str = "old secret text";

fn marked(mut o: StoredObject) -> StoredObject {
    o.envelope.sensitive = vec!["pd:Health".into()];
    o
}

fn with_text(mut o: StoredObject, text: &str) -> StoredObject {
    o.content["text"] = serde_json::json!(text);
    o
}

/// The blocker of the review: this used to leave `(Null, {})` everywhere, reported as accepted.
#[tokio::test]
async fn taking_the_mark_off_brings_the_real_object_everywhere() {
    let env = Env::start();
    let (store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let o = with_text(note("real name", 8), OLD);
    let id = o.envelope.id;
    store.put(marked(o.clone()), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(up.withheld, 1);
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());

    store.put(with_text(o, "open now"), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.problems.len()), (1, 0));
    for held in [
        env.hub().core().store().get(id).await.unwrap().unwrap(),
        store.get(id).await.unwrap().unwrap(),
    ] {
        assert_eq!(held.content["text"], "open now");
        assert_eq!(held.envelope.name, "real name");
        assert_eq!(held.envelope.object_type, "Note");
        assert_eq!(held.content_type, "application/json");
    }
    reader.sync_once(reader_store.as_ref()).await.unwrap();
    let got = reader_store.get(id).await.unwrap().unwrap();
    assert_eq!(got.content["text"], "open now");
    assert_eq!(got.envelope.name, "real name");
}

/// A large object too: only the stub travels, and a new reader gets nothing of the old copy.
#[tokio::test]
async fn a_mark_removes_the_copy_everywhere_and_nothing_old_goes_out_any_more() {
    let mut env = Env::start();
    let (store, early_store) = (env.store("owner").await, env.store("early").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let early = session(&env, notes_manifest("early")).await;
    let o = with_text(note("real name", 8), OLD);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    early.sync_once(early_store.as_ref()).await.unwrap();
    assert!(early_store.get(id).await.unwrap().is_some());

    // Marked, and above every limit: still only a withheld entry, no problem.
    let huge = "x".repeat(MAX_OBJECT_BYTES + 1);
    store.put(marked(with_text(o, &huge)), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(
        (up.withheld, up.problems.len()),
        (1, 0),
        "{:?}",
        up.problems
    );
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());

    // A reader that had it loses it, through a delete with no payload.
    let down = early.sync_once(early_store.as_ref()).await.unwrap();
    assert_eq!(
        (down.applied, down.failed.len()),
        (1, 0),
        "{:?}",
        down.failed
    );
    assert!(early_store.get(id).await.unwrap().is_none());

    // The list survives a restart: a new reader gets that it happened, and nothing of what it held.
    drop((owner, early));
    env.hub.take().unwrap().stop().unwrap();
    env.hub = Some(Daemon::spawn(config(&env.dir)).unwrap());
    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.unwrap();
    let kinds: Vec<_> = page.activities.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ActivityKind::Create, ActivityKind::Delete]);
    for a in &page.activities {
        assert!(a.payload.is_null(), "{:?}", a.payload);
        let entry = &page.notes[&a.id];
        assert_eq!(entry.original_at, None);
        assert_ne!(entry.envelope.name, "real name");
    }
    let late_store = env.store("late").await;
    let down = late.sync_once(late_store.as_ref()).await.unwrap();
    assert_eq!(down.failed.len(), 0, "{:?}", down.failed);
    assert!(late_store.get(id).await.unwrap().is_none());
}

/// After the mark is off again, what is new is delivered in full, and what is old stays out.
#[tokio::test]
async fn only_the_entries_from_before_a_mark_stay_out() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = with_text(note("name", 8), OLD);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    store.put(marked(o.clone()), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    store.put(with_text(o, "open now"), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let page = reader.pull(None, 0).await.unwrap();
    assert_eq!(page.activities.len(), 3);
    assert!(page.activities[0].payload.is_null() && page.activities[1].payload.is_null());
    assert!(!page.activities[2].payload.is_null());
    assert!(!format!("{:?}", page.activities).contains(OLD));
    let own = env.store("reader").await;
    assert_eq!(
        reader.sync_once(own.as_ref()).await.unwrap().failed.len(),
        0
    );
    assert_eq!(
        own.get(id).await.unwrap().unwrap().content["text"],
        "open now"
    );
}

/// A log-only entry about a sensitive object does not leave the program, nor does it block the log.
#[tokio::test]
async fn log_entries_about_a_sensitive_object_stay_home() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = marked(note("secret", 8));
    let id = o.envelope.id;
    store.put(o, "ada").await.unwrap();
    store
        .log(
            ActivityKind::Read,
            id,
            "ada",
            serde_json::json!({"note": OLD}),
        )
        .await
        .unwrap();
    let plain = note("plain", 8);
    let plain_id = plain.envelope.id;
    store.put(plain, "ada").await.unwrap();

    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.withheld, up.pushed, up.accepted), (2, 2, 1));
    let unsynced = ActivityFilter {
        unsynced_only: true,
        ..Default::default()
    };
    assert_eq!(
        store.activities(&unsynced).await.unwrap().len(),
        0,
        "moved past"
    );
    assert!(
        env.hub()
            .core()
            .store()
            .get(plain_id)
            .await
            .unwrap()
            .is_some()
    );
}
