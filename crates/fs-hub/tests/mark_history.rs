//! End-to-end: what a mark means for the past of an object: log entries and unsynced history stay on the
//! device, and a mark reaches the hub's log without a copy there.

mod common;

use common::{Env, note, notes_manifest, session};
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

/// The first sync of a log that holds the plain version before the mark: none of it leaves the device.
#[tokio::test]
async fn unsynced_history_of_a_marked_object_stays_home() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = with_text(note("real name", 8), OLD);
    let mut updated = o.clone();
    store.put(o.clone(), "ada").await.unwrap();
    updated.content["text"] = serde_json::json!("newer secret");
    store.put(updated, "ada").await.unwrap();
    store.put(marked(o), "ada").await.unwrap();

    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.withheld, up.accepted), (3, 0));
    let hub_log = env.hub().core().store();
    let log = hub_log
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(log.len(), 0, "{log:?}");
}

/// The hub holds no copy any more, its log does: a mark reaches that too.
#[tokio::test]
async fn a_mark_reaches_the_log_when_the_hub_holds_no_copy() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = with_text(note("real name", 8), OLD);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    store.delete(id, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());
    store.put(marked(o), "ada").await.unwrap();
    assert_eq!(owner.sync_once(store.as_ref()).await.unwrap().withheld, 1);

    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.unwrap();
    let kinds: Vec<_> = page.activities.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ActivityKind::Delete]);
    assert!(!format!("{:?}{:?}", page.activities, page.notes).contains(OLD));
    assert!(!format!("{:?}{:?}", page.activities, page.notes).contains("real name"));
}

/// Marked and deleted before the first sync: nothing says it was sensitive any more but the log.
#[tokio::test]
async fn marked_and_deleted_before_the_first_sync_stays_home() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = with_text(note("real name", 8), OLD);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    store.put(marked(o), "ada").await.unwrap();
    store.delete(id, "ada").await.unwrap();

    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(up.accepted, 0, "{up:?}");
    let hub_log = env.hub().core().store();
    let log = hub_log
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(log.len(), 0, "{log:?}");

    // And an object that was never marked still goes, delete included.
    let plain = note("plain", 8);
    let plain_id = plain.envelope.id;
    store.put(plain, "ada").await.unwrap();
    store.delete(plain_id, "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(up.accepted, 2, "{up:?}");
}
