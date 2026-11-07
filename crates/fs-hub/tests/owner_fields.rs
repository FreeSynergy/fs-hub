//! End-to-end: the program that first wrote an object decides the fields a reader never gets; what a
//! reader never gets of the rest does not reach it, and going round loses none of it.

mod common;

use common::{Env, note, notes_manifest, session};
use fs_objects::traits::{ActivityLog, ObjectRepository};
use fs_objects::{ActivityFilter, ActivityKind, Blob, StoredObject};

fn recipients(mut o: StoredObject, to: &[&str]) -> StoredObject {
    o.envelope.to = to.iter().map(ToString::to_string).collect();
    o
}

#[tokio::test]
async fn an_owner_changes_and_removes_them() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let mut o = recipients(note("n", 8), &["bob", "eve"]);
    o.envelope.attributed_to = Some("ada".into());
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    let hub = env.hub().core().store();

    let mut removed_eve = recipients(o.clone(), &["bob"]);
    store.put(removed_eve.clone(), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.skipped), (1, 0), "{up:?}");
    assert_eq!(hub.get(id).await.unwrap().unwrap().envelope.to, ["bob"]);

    removed_eve.envelope.to.clear();
    removed_eve.envelope.attributed_to = None;
    store.put(removed_eve, "ada").await.unwrap();
    assert_eq!(owner.sync_once(store.as_ref()).await.unwrap().accepted, 1);
    let held = hub.get(id).await.unwrap().unwrap();
    assert_eq!(
        (held.envelope.to.len(), held.envelope.attributed_to),
        (0, None)
    );
}

/// Another program's push does not decide them, whatever it holds.
#[tokio::test]
async fn another_programs_blanks_are_kept() {
    let env = Env::start();
    let (store, other_store) = (env.store("owner").await, env.store("other").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let other = session(&env, notes_manifest("other")).await;
    let o = recipients(note("n", 8), &["bob", "eve"]);
    let id = o.envelope.id;
    store.put(o, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    other.sync_once(other_store.as_ref()).await.unwrap();
    let mut copy = other_store.get(id).await.unwrap().unwrap();
    copy.content["text"] = serde_json::json!("edited");
    other_store.put(copy, "bob").await.unwrap();
    assert_eq!(
        other
            .sync_once(other_store.as_ref())
            .await
            .unwrap()
            .accepted,
        1
    );
    let held = env.hub().core().store().get(id).await.unwrap().unwrap();
    assert_eq!(held.envelope.to, ["bob", "eve"]);
}

fn with_all_of_it() -> StoredObject {
    note("n", 8)
        .with_index("person", "https://example.org/eve")
        .with_index("kind", "note")
        .with_original(Blob {
            media_type: "message/rfc822".into(),
            bytes: b"Bcc: eve@example.org".to_vec(),
        })
}

/// The original, the index keys that name people, and log-only payloads stay home, and come to no harm.
#[tokio::test]
async fn original_index_and_log_payloads_do_not_reach_a_reader_and_survive_the_round() {
    let env = Env::start();
    let (store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let o = with_all_of_it();
    let id = o.envelope.id;
    store.put(o, "ada").await.unwrap();
    store
        .log(
            ActivityKind::Invite,
            id,
            "ada",
            serde_json::json!({"who": "eve@example.org"}),
        )
        .await
        .unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    let hub = env.hub().core().store();
    let at_hub = hub.get(id).await.unwrap().unwrap();
    assert!(at_hub.original.is_some() && at_hub.index.contains_key("person"));

    let page = reader.pull(None, 0).await.unwrap();
    let text = format!("{:?}{:?}", page.activities, page.notes);
    assert!(
        !text.contains("eve") && !text.contains("\"original\""),
        "{text}"
    );
    assert!(page.activities[1].payload.is_null(), "a log-only payload");

    reader.sync_once(reader_store.as_ref()).await.unwrap();
    let mut copy = reader_store.get(id).await.unwrap().unwrap();
    assert!(copy.original.is_none() && !copy.index.contains_key("person"));
    assert_eq!(copy.index["kind"], ["note"], "what a reader needs");
    let local_log = reader_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(local_log.len(), 2);

    // Unchanged it is no change, changed it takes nothing away.
    let echo = reader.push(local_log[..1].to_vec()).await.unwrap();
    assert_eq!(echo.accepted, 0, "{echo:?}");
    copy.content["text"] = serde_json::json!("edited");
    reader_store.put(copy, "bob").await.unwrap();
    assert_eq!(
        reader
            .sync_once(reader_store.as_ref())
            .await
            .unwrap()
            .accepted,
        1
    );
    let at_hub = hub.get(id).await.unwrap().unwrap();
    assert_eq!(at_hub.content["text"], "edited");
    assert_eq!(at_hub.index["person"], ["https://example.org/eve"]);
    assert!(at_hub.original.is_some());
    owner.sync_once(store.as_ref()).await.unwrap();
    let at_owner = store.get(id).await.unwrap().unwrap();
    assert_eq!(at_owner.content["text"], "edited");
    assert_eq!(at_owner.index["person"], ["https://example.org/eve"]);
    assert!(at_owner.original.is_some());
}
