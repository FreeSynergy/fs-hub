//! End-to-end: entries in the hub's log from before the limits; the hub takes nothing like them now.

mod common;

use common::{Env, note, notes_manifest, session};
use fs_hub_client::MAX_HUB_MESSAGE_BYTES;
use fs_store::traits::ObjectRepository;
use fs_store::{Activity, ActivityFilter, ActivityKind};

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

/// An entry from before these limits: no message holds it. The reader sees that and goes on.
#[tokio::test]
async fn an_old_entry_too_large_for_a_message_does_not_block_the_reader() {
    let env = Env::start();
    let hub_store = env.hub().core().store();
    let old = note("old", MAX_HUB_MESSAGE_BYTES / 2 + (1 << 20));
    let id = old.envelope.id;
    let mut changed = old.clone();
    hub_store.put(old, "wiki").await.unwrap();
    changed.content["text"] = serde_json::json!("y".repeat(MAX_HUB_MESSAGE_BYTES / 2 + (1 << 20)));
    hub_store.put(changed, "wiki").await.unwrap();
    hub_store.put(note("next", 8), "wiki").await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let store = env.store("reader").await;
    let report = reader
        .sync_once(store.as_ref())
        .await
        .expect("no dead session");
    assert_eq!(report.pulled, 3);
    assert_eq!(report.failed.len(), 1, "{:?}", report.failed);
    assert!(
        report.failed[0].contains("held back"),
        "{}",
        report.failed[0]
    );
    assert_eq!(
        reader.sync_once(store.as_ref()).await.unwrap().pulled,
        0,
        "moved past"
    );

    // The summary is what still fits.
    let envelopes = reader
        .pull_envelopes(Some(first_id(&hub_store).await), 10)
        .await
        .unwrap();
    assert_eq!(envelopes.notes[&envelopes.activities[0].id].envelope.id, id);
}

async fn first_id(store: &std::sync::Arc<dyn fs_store::Store>) -> fs_store::ActivityId {
    let log: Vec<Activity> = store.activities(&ActivityFilter::default()).await.unwrap();
    assert_eq!(log[0].kind, ActivityKind::Create);
    log[0].id
}
