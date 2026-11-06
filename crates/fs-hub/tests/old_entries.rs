//! End-to-end: entries in the hub's log from before the limits; the hub takes nothing like them now.

mod common;

use common::{Env, note, notes_manifest, session};
use fs_hub_client::MAX_HUB_MESSAGE_BYTES;
use fs_store::traits::ObjectRepository;

/// An entry from before these limits, of 80 MiB: only the raised limit lets the client read it.
#[tokio::test]
async fn an_old_entry_above_the_default_limit_still_arrives() {
    let env = Env::start();
    let hub_store = env.hub().core().store();
    let old = note("old", 40 << 20);
    let id = old.envelope.id;
    let mut changed = old.clone();
    hub_store.put(old, "wiki").await.unwrap();
    changed.content["text"] = serde_json::json!("y".repeat(40 << 20));
    hub_store.put(changed, "wiki").await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let store = env.store("reader").await;
    let report = reader
        .sync_once(store.as_ref())
        .await
        .expect("no dead session");
    assert_eq!(
        (report.applied, report.failed.len()),
        (2, 0),
        "{:?}",
        report.failed
    );
    let got = store.get(id).await.unwrap().unwrap();
    assert!(got.content["text"].as_str().unwrap().starts_with('y'));
}

const ABOVE_A_MESSAGE: usize = MAX_HUB_MESSAGE_BYTES + (1 << 20);

/// `after` alone is too large for any message: the reader sees that and goes on.
#[tokio::test]
async fn an_old_entry_too_large_for_a_message_does_not_block_the_reader() {
    let env = Env::start();
    let hub_store = env.hub().core().store();
    let old = note("old", ABOVE_A_MESSAGE);
    let id = old.envelope.id;
    hub_store.put(old, "wiki").await.unwrap();
    hub_store.put(note("next", 8), "wiki").await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let store = env.store("reader").await;
    let report = reader
        .sync_once(store.as_ref())
        .await
        .expect("no dead session");
    assert_eq!((report.pulled, report.applied), (2, 1));
    assert_eq!(report.failed.len(), 1, "{:?}", report.failed);
    assert!(
        report.failed[0].contains("held back"),
        "{}",
        report.failed[0]
    );
    assert_eq!(reader.sync_once(store.as_ref()).await.unwrap().pulled, 0);

    // The summary is what still fits.
    let another = session(&env, notes_manifest("another")).await;
    let page = another.pull_envelopes(None, 1).await.unwrap();
    assert_eq!(page.entries[0].envelope.id, id);
}

/// The whole entry is too large, its `after` is not: a reader must not stay on the old version.
#[tokio::test]
async fn an_old_update_arrives_without_its_large_before_image() {
    let env = Env::start();
    let hub_store = env.hub().core().store();
    let large = note("large", ABOVE_A_MESSAGE);
    let id = large.envelope.id;
    let mut small = large.clone();
    hub_store.put(large, "wiki").await.unwrap();
    small.content = serde_json::json!({ "text": "small" });
    hub_store.put(small, "wiki").await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let store = env.store("reader").await;
    let report = reader.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(
        report.failed.len(),
        1,
        "only the create: {:?}",
        report.failed
    );
    let got = store.get(id).await.unwrap().expect("not stale");
    assert_eq!(got.content["text"], "small");
}

/// A delete whose `before` is too large is still carried out.
#[tokio::test]
async fn an_old_delete_is_carried_out() {
    let env = Env::start();
    let store = env.store("reader").await;
    let reader = session(&env, notes_manifest("reader")).await;
    let hub_store = env.hub().core().store();
    let mut small = note("small", 8);
    let id = small.envelope.id;
    hub_store.put(small.clone(), "wiki").await.unwrap();
    reader.sync_once(store.as_ref()).await.unwrap();
    assert!(store.get(id).await.unwrap().is_some());

    small.content = serde_json::json!({ "text": "x".repeat(ABOVE_A_MESSAGE) });
    hub_store.put(small, "wiki").await.unwrap();
    hub_store.delete(id, "wiki").await.unwrap();
    let report = reader.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(
        report.failed.len(),
        1,
        "only the update: {:?}",
        report.failed
    );
    assert!(store.get(id).await.unwrap().is_none());
}
