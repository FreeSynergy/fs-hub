//! End-to-end: the fields that name a person do not reach a reader, and going round loses nothing.
//!
//! PROVISIONAL, see `delivery.rs` in `fs-hub-proto`.
#![allow(clippy::unused_async_trait_impl)] // the capnp `change_listener::Server` trait dictates `async fn`

mod common;

use std::rc::Rc;
use std::time::Duration;

use capnp_rpc::new_client;
use common::raw::{audience_of, register};
use common::{Env, note_of_ada, notes_manifest, session};
use fs_hub_proto::common_capnp::{change, change_listener};
use fs_objects::traits::{ActivityLog, ObjectRepository};
use fs_objects::{ActivityFilter, StoredObject};
use tokio::sync::mpsc;

fn assert_blank(o: &StoredObject) {
    assert_eq!(o.envelope.attributed_to, None);
    let e = &o.envelope;
    for list in [&e.to, &e.cc, &e.bto, &e.bcc, &e.audience] {
        assert_eq!(list.len(), 0, "{list:?}");
    }
}

/// The owner writes, a reader pulls and changes it, it goes back: the hub and the owner keep every
/// original value, and everybody has the new content.
#[tokio::test]
async fn a_full_round_trip_overwrites_nothing() {
    let env = Env::start();
    let (owner_store, reader_store) = (env.store("owner").await, env.store("reader").await);
    let owner = session(&env, notes_manifest("owner")).await;
    let reader = session(&env, notes_manifest("reader")).await;
    let original = note_of_ada("n", 8);
    let id = original.envelope.id;
    let originals = original.envelope.clone();
    owner_store.put(original, "ada").await.unwrap();
    owner.sync_once(owner_store.as_ref()).await.unwrap();
    let hub = env.hub().core().store();
    let log_len = |s: std::sync::Arc<dyn fs_objects::Store>| async move {
        s.activities(&ActivityFilter::default())
            .await
            .unwrap()
            .len()
    };

    let same_fields = |o: &StoredObject| {
        assert_eq!(o.envelope.attributed_to, originals.attributed_to);
        assert_eq!(o.envelope.to, originals.to);
        assert_eq!(o.envelope.cc, originals.cc);
        assert_eq!(o.envelope.bto, originals.bto);
        assert_eq!(o.envelope.bcc, originals.bcc);
        assert_eq!(o.envelope.audience, originals.audience);
    };

    // The reader's copy is blank.
    reader.sync_once(reader_store.as_ref()).await.unwrap();
    let copy = reader_store.get(id).await.unwrap().unwrap();
    assert_blank(&copy);
    assert_eq!(copy.content["text"], "x".repeat(8));

    // Pushing it back unchanged is no change: `same_content` does not flip.
    let before = log_len(hub.clone()).await;
    let reader_log = reader_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(reader_log.len(), 1);
    let outcome = reader.push(reader_log).await.unwrap();
    assert_eq!((outcome.accepted, outcome.skipped), (0, 1));
    assert_eq!(
        log_len(hub.clone()).await,
        before,
        "no new entry at the hub"
    );
    same_fields(&hub.get(id).await.unwrap().unwrap());

    // The reader changes the content.
    let mut changed = copy;
    changed.content["text"] = serde_json::json!("changed by the reader");
    reader_store.put(changed, "reader").await.unwrap();
    assert_eq!(
        reader
            .sync_once(reader_store.as_ref())
            .await
            .unwrap()
            .accepted,
        1
    );
    let at_hub = hub.get(id).await.unwrap().unwrap();
    assert_eq!(at_hub.content["text"], "changed by the reader");
    same_fields(&at_hub);

    // Back at the owner: new content, its own values.
    let down = owner.sync_once(owner_store.as_ref()).await.unwrap();
    assert_eq!(down.applied, 1);
    let at_owner = owner_store.get(id).await.unwrap().unwrap();
    assert_eq!(at_owner.content["text"], "changed by the reader");
    same_fields(&at_owner);

    // It settles: nothing travels any more, and the owner's own change still has its values.
    let hub_len = log_len(hub.clone()).await;
    assert_eq!(
        owner.sync_once(owner_store.as_ref()).await.unwrap().pushed,
        0
    );
    assert_eq!(
        reader
            .sync_once(reader_store.as_ref())
            .await
            .unwrap()
            .applied,
        0
    );
    assert_eq!(log_len(hub.clone()).await, hub_len);
    assert_blank(&reader_store.get(id).await.unwrap().unwrap());
    same_fields(&hub.get(id).await.unwrap().unwrap());
}

#[tokio::test]
async fn the_wire_holds_none_of_them_also_in_the_images() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("owner").await;
            let owner = session(&env, notes_manifest("owner")).await;
            let o = note_of_ada("n", 8);
            let mut changed = o.clone();
            store.put(o, "ada").await.unwrap();
            changed.content["text"] = serde_json::json!("v2");
            store.put(changed, "ada").await.unwrap();
            owner.sync_once(store.as_ref()).await.unwrap();

            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let answer = reader.pull(None, 0, false).await;
            assert_eq!(answer.activities.len(), 2);
            for a in &answer.activities {
                assert_eq!(a.audience, Vec::<String>::new());
                assert!(a.payload_len > 0);
                assert!(!a.payload.contains("example.org"), "{}", a.payload);
            }
            let summaries = reader.pull(None, 0, true).await;
            assert_eq!(summaries.activities[0].audience, Vec::<String>::new());
        })
        .await;
}

struct Heard(mpsc::UnboundedSender<Vec<String>>);

impl change_listener::Server for Heard {
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> Result<(), capnp::Error> {
        match params.get()?.get_change()?.which()? {
            change::Which::Created(e) | change::Which::Updated(e) => {
                let _ = self.0.send(audience_of(e?));
            }
            change::Which::Deleted(_) => {}
        }
        Ok(())
    }
}

#[tokio::test]
async fn live_notices_hold_none_of_them() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let (tx, mut heard) = mpsc::unbounded_channel();
            let mut request = reader.session.subscribe_request();
            request.get().set_listener(new_client(Heard(tx)));
            let _subscription = request.send().promise.await.unwrap();

            let store = env.store("owner").await;
            let owner = session(&env, notes_manifest("owner")).await;
            let o = note_of_ada("n", 8);
            let mut updated = o.clone();
            store.put(o, "ada").await.unwrap();
            updated.content["text"] = serde_json::json!("v2");
            store.put(updated, "ada").await.unwrap();
            owner.sync_once(store.as_ref()).await.unwrap();
            for which in ["created", "updated"] {
                let got = tokio::time::timeout(Duration::from_secs(5), heard.recv())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(got, Vec::<String>::new(), "{which}");
            }
        })
        .await;
}
