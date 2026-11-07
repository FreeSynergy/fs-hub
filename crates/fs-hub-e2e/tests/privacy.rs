//! What must stay home stays home: marked objects, role assignments, and entries without state.

mod common;

use common::{Env, calendar, held, id, mark_sensitive, people, settle, tasks};
use fs_hub_client::connect_at;
use fs_store::traits::ObjectRepository;
use fs_store::{Activity, ActivityId, ActivityKind, Envelope, StoredObject};

#[tokio::test]
async fn a_marked_object_stays_away_from_the_hub_and_its_copies_go() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let p = people::start(&env, "people").await;
            let t = tasks::start(&env, "tasks").await;
            let c = calendar::start(&env, "calendar").await;
            let audit_store = env.store("audit").await;
            let audit = env.session(common::tasks::manifest("audit")).await;
            let all = [
                (&p.session, p.store.as_ref()),
                (&t.session, t.store.as_ref()),
                (&c.session, c.store.as_ref()),
                (&audit, audit_store.as_ref()),
            ];

            // Marked before it ever left: never collected, never proposed, nobody has a copy.
            let secret = p
                .create("u9", "Alex", "alex@example.org", "+49 30 7654321")
                .await;
            mark_sensitive(&p.store, secret, "alex").await;
            // Marked after it was shared: the copies go, the owner keeps its own.
            let task = t.create("t1", "Appointment").await;
            settle(&all).await;
            assert!(held(&c.store, task).await.is_some(), "shared first");

            mark_sensitive(&t.store, task, "me").await;
            settle(&all).await;
            assert!(held(&p.store, secret).await.is_some(), "the owner keeps it");
            assert!(held(&t.store, task).await.is_some(), "the owner keeps it");
            for store in [&c.store, &audit_store, &t.store] {
                assert!(held(store, secret).await.is_none(), "never collected");
            }
            assert!(
                held(&c.store, task).await.is_none(),
                "taken off the hub, copies go"
            );
            assert!(held(&audit_store, task).await.is_none());
            assert!(p.session.proposals().await.unwrap().is_empty());
        })
        .await;
}

#[tokio::test]
async fn nobody_reads_role_assignments() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let p = people::start(&env, "people").await;
            let c = calendar::start(&env, "calendar").await;

            // The type cannot be declared for reading, by any program ...
            for manifest in [
                people::manifest("greedy").read(&["Profile", "Relationship"]),
                calendar::manifest("greedy2").read(&["Relationship"]),
            ] {
                assert!(connect_at(&env.socket(), manifest).await.is_err());
            }
            // ... and what people wrote of that type reaches no program.
            let link = StoredObject::new(
                Envelope::new("Relationship", "customer of Ada"),
                "application/json",
                serde_json::json!({"relationship": "customer"}),
            );
            let link_id = p.store.put(link, "people").await.unwrap().object;
            settle(&[
                (&p.session, p.store.as_ref()),
                (&c.session, c.store.as_ref()),
            ])
            .await;
            assert!(held(&c.store, link_id.as_uuid()).await.is_none());
            assert_eq!(common::count(&c.store, "Relationship").await, 0);
        })
        .await;
}

#[tokio::test]
async fn an_entry_without_state_deletes_no_copy() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let t = tasks::start(&env, "tasks").await;
            let c = calendar::start(&env, "calendar").await;
            let all = [(&t.session, t.store.as_ref()), (&c.session, c.store.as_ref())];
            let task = t.create("t1", "Kept").await;
            settle(&all).await;
            assert!(held(&c.store, task).await.is_some());

            // What `purge_object` of fs-store 0.4 leaves for later writes. fs-store 0.2 cannot purge, so
            // the entry is handed to the hub as it would be.
            let bare = |kind| Activity {
                id: ActivityId::new(),
                kind,
                object: id(task),
                actor: String::new(),
                at: chrono::Utc::now(),
                target: None,
                payload: serde_json::json!({"redacted": true, "before": "redacted", "after": "redacted"}),
                synced: false,
            };
            let outcome = t
                .session
                .push([ActivityKind::Delete, ActivityKind::Update].map(bare).to_vec())
                .await
                .unwrap();
            assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
            assert_eq!((outcome.accepted, outcome.skipped), (0, 2));
            settle(&all).await;
            assert!(held(&c.store, task).await.is_some(), "the copy stays");

            // A real delete still removes it.
            t.delete(task).await;
            settle(&all).await;
            assert!(held(&c.store, task).await.is_none());
        })
        .await;
}
