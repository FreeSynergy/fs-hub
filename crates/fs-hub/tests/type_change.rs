//! End-to-end: naming another type does not get round `writes`.

mod common;

use common::{Env, contacts_manifest, note, notes_manifest, session};
use fs_store::traits::ObjectRepository;
use fs_store::{Envelope, StoredObject};

fn profile(name: &str) -> StoredObject {
    StoredObject::new(
        Envelope::new("Profile", name),
        "application/json",
        serde_json::json!({"text": name}),
    )
}

fn as_note(mut o: StoredObject) -> StoredObject {
    o.envelope.object_type = "Note".into();
    o.content["text"] = serde_json::json!("overwritten");
    o
}

#[tokio::test]
async fn a_program_cannot_overwrite_a_card_by_calling_it_a_note() {
    let env = Env::start();
    let (store, wiki_store) = (env.store("contacts").await, env.store("wiki").await);
    let contacts = session(&env, contacts_manifest("contacts")).await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    let card = profile("Ada");
    let id = card.envelope.id;
    store.put(card.clone(), "ada").await.unwrap();
    contacts.sync_once(store.as_ref()).await.unwrap();

    wiki_store.put(as_note(card), "x").await.unwrap();
    let up = wiki.sync_once(wiki_store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.withheld), (0, 1), "{up:?}");
    let held = env.hub().core().store().get(id).await.unwrap().unwrap();
    assert_eq!(held.envelope.object_type, "Profile");
    assert_eq!(held.content["text"], "Ada");
}

/// Also when the hub holds no copy any more: its log knows the type.
#[tokio::test]
async fn nor_a_deleted_one() {
    let env = Env::start();
    let (store, wiki_store) = (env.store("contacts").await, env.store("wiki").await);
    let contacts = session(&env, contacts_manifest("contacts")).await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    let card = profile("Ada");
    let id = card.envelope.id;
    store.put(card.clone(), "ada").await.unwrap();
    contacts.sync_once(store.as_ref()).await.unwrap();
    store.delete(id, "ada").await.unwrap();
    contacts.sync_once(store.as_ref()).await.unwrap();

    wiki_store.put(as_note(card), "x").await.unwrap();
    let up = wiki.sync_once(wiki_store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.withheld), (0, 1), "{up:?}");
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());

    // A type the program may write works as ever.
    let own = note("own", 8);
    wiki_store.put(own, "x").await.unwrap();
    assert_eq!(
        wiki.sync_once(wiki_store.as_ref()).await.unwrap().accepted,
        1
    );
}
