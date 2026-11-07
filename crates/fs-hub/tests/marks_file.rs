//! End-to-end: the list of marked objects is noticed when it goes missing, is built over an older hub's
//! log, and is enforced against a restored `objects.db`.

mod common;

use common::{Env, config, note, notes_manifest, session};
use fs_hub::{Daemon, Error};
use fs_objects::traits::ObjectRepository;
use fs_objects::{ActivityKind, TursoStore};

const OLD: &str = "old secret text";

#[tokio::test]
async fn a_missing_list_stops_the_hub_from_starting() {
    let mut env = Env::start();
    env.hub.take().unwrap().stop().unwrap();
    let marks = env.dir.path().join("hub-data/marks.db");
    assert!(marks.exists());
    std::fs::remove_file(&marks).unwrap();
    let refused = Daemon::spawn(config(&env.dir));
    assert!(
        matches!(refused, Err(Error::MarksLost(_))),
        "started anyway"
    );
}

/// A hub from before the list: objects.db and nothing else.
#[tokio::test]
async fn over_an_older_log_every_deleted_object_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("hub-data");
    fs_interfaces::rpc::prepare_data_dir(&data).unwrap();
    let (deleted, kept);
    {
        let old = TursoStore::open(data.join("objects.db")).await.unwrap();
        let mut o = note("real name", 8);
        o.content["text"] = serde_json::json!(OLD);
        deleted = o.envelope.id;
        old.put(o, "wiki").await.unwrap();
        old.delete(deleted, "wiki").await.unwrap();
        let k = note("still here", 8);
        kept = k.envelope.id;
        old.put(k, "wiki").await.unwrap();
    }
    let env = Env {
        hub: Some(Daemon::spawn(config(&dir)).unwrap()),
        dir,
    };
    let marks = env.hub().core().marked();
    let listed = marks.find(deleted).await.unwrap().expect("listed");
    assert!(!listed.holds, "conservative: it may be an ordinary delete");
    assert_eq!(
        listed.origin, "wiki",
        "the origin is on the list, not only in the log"
    );
    assert!(marks.find(kept).await.unwrap().is_none());

    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.unwrap();
    let text = format!("{:?}{:?}", page.activities, page.notes);
    assert!(!text.contains(OLD) && !text.contains("real name"), "{text}");
    let kinds: Vec<_> = page.activities.iter().map(|a| a.kind).collect();
    assert_eq!(kinds, [ActivityKind::Delete, ActivityKind::Create]);

    // The owner gets no delete from that list either, and it does not hold anything back.
    let wiki = session(&env, notes_manifest("wiki")).await;
    assert_eq!(wiki.pull(None, 0).await.unwrap().activities.len(), 0);
    let store = env.store("wiki").await;
    let again = note("n", 8);
    store.put(again.clone(), "ada").await.unwrap();
    assert_eq!(wiki.sync_once(store.as_ref()).await.unwrap().accepted, 1);
}

/// `objects.db` restored from an older backup: it has the copy again, the list says it must go.
#[tokio::test]
async fn at_start_a_mark_that_holds_takes_a_restored_copy_off() {
    let mut env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let o = note("real name", 8);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    let mut secret = o.clone();
    secret.envelope.sensitive = vec!["pd:Health".into()];
    store.put(secret, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    drop(owner);
    env.hub.take().unwrap().stop().unwrap();
    {
        let objects = TursoStore::open(env.dir.path().join("hub-data/objects.db"))
            .await
            .unwrap();
        objects.put(o, "owner").await.unwrap();
    }
    env.hub = Some(Daemon::spawn(config(&env.dir)).unwrap());
    let hub = env.hub().core().store();
    assert!(hub.get(id).await.unwrap().is_none(), "taken off at start");
    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.unwrap();
    assert!(!format!("{:?}", page.activities).contains("real name"));
}

/// People's data: a directory others can reach is refused.
#[tokio::test]
async fn a_data_directory_others_can_reach_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("hub-data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, PermissionsExt::from_mode(0o755)).unwrap();
    assert!(Daemon::spawn(config(&dir)).is_err());
}
