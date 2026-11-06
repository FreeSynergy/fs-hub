//! End-to-end: a mark is the program's own: nothing pulled undoes it, and a chosen object id stops nobody.

mod common;

use common::{Env, note, notes_manifest, only_reads_notes, session};
use fs_store::StoredObject;
use fs_store::traits::ObjectRepository;
use fs_store::{ActivityFilter, ObjectId};

fn marked(mut o: StoredObject) -> StoredObject {
    o.envelope.sensitive = vec!["pd:Health".into()];
    o
}

fn with_text(mut o: StoredObject, text: &str) -> StoredObject {
    o.content["text"] = serde_json::json!(text);
    o
}

/// What this program marked stays as it is, whatever is pulled.
#[tokio::test]
async fn nothing_pulled_overwrites_or_deletes_a_local_mark() {
    let env = Env::start();
    let (store, holder_store) = (env.store("owner").await, env.store("holder").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let holder = session(&env, only_reads_notes("holder")).await;
    let o = with_text(note("n", 8), "v1");
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    holder_store
        .put(marked(with_text(o.clone(), "mine")), "bob")
        .await
        .unwrap();

    let first = holder.sync_once(holder_store.as_ref()).await.unwrap();
    assert_eq!(first.applied, 0, "{first:?}");
    store.put(with_text(o, "v2"), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(
        holder
            .sync_once(holder_store.as_ref())
            .await
            .unwrap()
            .applied,
        0
    );
    store.delete(id, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(
        holder
            .sync_once(holder_store.as_ref())
            .await
            .unwrap()
            .applied,
        0
    );

    let held = holder_store.get(id).await.unwrap().unwrap();
    assert_eq!(held.envelope.sensitive, ["pd:Health"]);
    assert_eq!(held.content["text"], "mine");
}

/// A program chooses object ids, so one can pick the id of a log entry. That stops nobody's `pull`.
#[tokio::test]
async fn an_object_at_the_id_of_a_log_entry_stops_nothing() {
    let env = Env::start();
    let store = env.store("owner").await;
    let owner = session(&env, notes_manifest("owner")).await;
    store.put(note("first", 8), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    let hub = env.hub().core().store();
    let entry = hub.activities(&ActivityFilter::default()).await.unwrap()[0].id;

    let (attacker_store, attacker) = (
        env.store("attacker").await,
        session(&env, notes_manifest("attacker")).await,
    );
    let mut chosen = note("chosen", 8);
    chosen.envelope.id = ObjectId::from_uuid(entry.as_uuid()).unwrap();
    attacker_store.put(chosen.clone(), "x").await.unwrap();
    attacker.sync_once(attacker_store.as_ref()).await.unwrap();
    attacker_store.put(marked(chosen), "x").await.unwrap();
    assert_eq!(
        attacker
            .sync_once(attacker_store.as_ref())
            .await
            .unwrap()
            .withheld,
        1
    );

    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.expect("everybody can still pull");
    let first = page.activities.first().unwrap();
    assert_eq!(first.id, entry);
    assert!(
        page.notes[&entry].original_at.is_some(),
        "its time is its own"
    );
}

/// The owner may lift another program's mark, by its own mark and taking that off. Nobody else may.
#[tokio::test]
async fn the_owner_lifts_a_mark_and_a_third_program_cannot() {
    let env = Env::start();
    let (store, other_store) = (env.store("owner").await, env.store("other").await);
    let third_store = env.store("third").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let other = session(&env, notes_manifest("other")).await;
    let third = session(&env, notes_manifest("third")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    third.sync_once(third_store.as_ref()).await.unwrap();
    other.sync_once(other_store.as_ref()).await.unwrap();
    let copy = other_store.get(id).await.unwrap().unwrap();
    other_store.put(marked(copy), "bob").await.unwrap();
    other.sync_once(other_store.as_ref()).await.unwrap();
    let hub = env.hub().core().store();

    // A third program takes a mark off its own copy: refused, the mark holds.
    let copy = third_store.get(id).await.unwrap().unwrap();
    third_store.put(marked(copy.clone()), "c").await.unwrap();
    third_store.put(copy, "c").await.unwrap();
    let up = third.sync_once(third_store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.problems.len()), (0, 1), "{up:?}");
    assert_eq!(up.problems[0].status, 409);
    assert!(hub.get(id).await.unwrap().is_none());

    // The owner does: its object is back, at once.
    store.put(marked(o.clone()), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    store.put(with_text(o, "open again"), "ada").await.unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.problems.len()), (1, 0), "{up:?}");
    assert_eq!(
        hub.get(id).await.unwrap().unwrap().content["text"],
        "open again"
    );
}
