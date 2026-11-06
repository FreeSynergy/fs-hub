//! End-to-end: who may do what to a marked object. A mark takes the object off the hub only (provisional).

mod common;

use common::{Env, note, notes_manifest, only_reads_notes, session};
use fs_store::traits::ObjectRepository;
use fs_store::{ActivityKind, StoredObject};

const OLD: &str = "old secret text";

fn marked(mut o: StoredObject) -> StoredObject {
    o.envelope.sensitive = vec!["pd:Health".into()];
    o
}

fn with_text(mut o: StoredObject, text: &str) -> StoredObject {
    o.content["text"] = serde_json::json!(text);
    o
}

/// A reader changes its copy, not yet knowing of the mark.
#[tokio::test]
async fn a_stale_edit_does_not_bring_the_secret_back() {
    let env = Env::start();
    let (store, stale_store) = (env.store("owner").await, env.store("stale").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let stale = session(&env, notes_manifest("stale")).await;
    let o = with_text(note("real name", 8), OLD);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    stale.sync_once(stale_store.as_ref()).await.unwrap();
    store.put(marked(o), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();

    let copy = stale_store.get(id).await.unwrap().unwrap();
    stale_store
        .put(with_text(copy, "edited"), "bob")
        .await
        .unwrap();
    let report = stale.sync_once(stale_store.as_ref()).await.unwrap();
    assert_eq!(
        (report.accepted, report.problems.len()),
        (0, 1),
        "{report:?}"
    );
    assert_eq!(report.problems[0].status, 409);
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());
    assert!(
        stale_store.get(id).await.unwrap().is_none(),
        "its copy goes"
    );

    // The owner still has it, marked, and hears of nothing.
    let down = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(down.applied, 0);
    let held = store.get(id).await.unwrap().unwrap();
    assert_eq!(held.envelope.sensitive, ["pd:Health"]);
    assert_eq!(held.content["text"], OLD);
}

/// A program that declared no `writes` destroys nothing, by a mark or by a delete.
#[tokio::test]
async fn without_writes_nothing_goes() {
    for how in ["mark", "delete"] {
        let env = Env::start();
        let (store, reader_store) = (env.store("owner").await, env.store("reader").await);
        let owner = session(&env, notes_manifest("owner")).await;
        let reader = session(&env, only_reads_notes("reader")).await;
        let o = note("n", 8);
        let id = o.envelope.id;
        store.put(o, "ada").await.unwrap();
        owner.sync_once(store.as_ref()).await.unwrap();
        reader.sync_once(reader_store.as_ref()).await.unwrap();

        if how == "mark" {
            let copy = reader_store.get(id).await.unwrap().unwrap();
            reader_store.put(marked(copy), "bob").await.unwrap();
        } else {
            reader_store.delete(id, "bob").await.unwrap();
        }
        let up = reader.sync_once(reader_store.as_ref()).await.unwrap();
        assert_eq!(up.withheld, 1, "{how}: {up:?}");
        assert!(
            env.hub().core().store().get(id).await.unwrap().is_some(),
            "{how}"
        );
        owner.sync_once(store.as_ref()).await.unwrap();
        assert!(store.get(id).await.unwrap().is_some(), "{how}");
        let late = session(&env, notes_manifest("late")).await;
        let page = late.pull(None, 0).await.unwrap();
        assert!(
            !page.activities[0].payload.is_null(),
            "{how}: nothing is listed"
        );
    }
}

/// Without a copy at the hub, too: its log is what the type comes from.
#[tokio::test]
async fn without_writes_a_mark_does_not_list_what_the_hub_no_longer_holds() {
    let env = Env::start();
    let (store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, only_reads_notes("reader")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    store.put(o.clone(), "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    store.delete(id, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    reader_store.put(marked(o), "bob").await.unwrap();
    reader.sync_once(reader_store.as_ref()).await.unwrap();

    let late = session(&env, notes_manifest("late")).await;
    let page = late.pull(None, 0).await.unwrap();
    assert_eq!(page.activities.len(), 2, "the old entries are as they were");
    assert!(!page.activities[0].payload.is_null());
}

/// A program that may write marks its copy: off the hub, readers lose it, the owner's object stays.
#[tokio::test]
async fn a_mark_from_another_program_never_deletes_the_owners_object() {
    let env = Env::start();
    let (store, other_store) = (env.store("owner").await, env.store("other").await);
    let early_store = env.store("early").await;
    let owner = session(&env, notes_manifest("owner")).await;
    let other = session(&env, notes_manifest("other")).await;
    let early = session(&env, notes_manifest("early")).await;
    let o = note("n", 8);
    let id = o.envelope.id;
    store.put(o, "ada").await.unwrap();
    owner.sync_once(store.as_ref()).await.unwrap();
    other.sync_once(other_store.as_ref()).await.unwrap();
    early.sync_once(early_store.as_ref()).await.unwrap();

    let copy = other_store.get(id).await.unwrap().unwrap();
    other_store.put(marked(copy), "bob").await.unwrap();
    assert_eq!(
        other
            .sync_once(other_store.as_ref())
            .await
            .unwrap()
            .withheld,
        1
    );
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());

    let page = owner.pull(None, 0).await.unwrap();
    assert_eq!(page.activities.len(), 0, "the owner gets no delete");
    owner.sync_once(store.as_ref()).await.unwrap();
    let kept = store.get(id).await.unwrap().unwrap();
    assert_eq!(kept.envelope.sensitive.len(), 0);

    let page = early.pull(None, 0).await.unwrap();
    assert_eq!(page.activities[0].kind, ActivityKind::Delete);
    early.sync_once(early_store.as_ref()).await.unwrap();
    assert!(early_store.get(id).await.unwrap().is_none(), "a copy goes");

    // While that mark holds, the owner's change stays with the owner, and it is told so.
    store
        .put(with_text(kept, "owner's edit"), "ada")
        .await
        .unwrap();
    let up = owner.sync_once(store.as_ref()).await.unwrap();
    assert_eq!((up.accepted, up.problems.len()), (0, 1), "{up:?}");
    assert!(env.hub().core().store().get(id).await.unwrap().is_none());
}
