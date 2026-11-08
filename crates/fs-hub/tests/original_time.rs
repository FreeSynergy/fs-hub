//! End-to-end: `originalAt` travels and stays; `originalActor` is delivered to nobody; a sensitive
//! object says neither (ADR 0029).
#![allow(clippy::assert_is_empty)] // tests: plain `is_empty()` asserts read clearer than comparing to typed empty arrays

mod common;

use std::time::Duration;

use common::raw::register;
use common::{Env, config, note, note_of_ada, notes_manifest, session};
use fs_hub::Daemon;
use fs_objects::ActivityFilter;
use fs_objects::traits::{ActivityLog, ObjectRepository};

#[tokio::test]
async fn original_at_arrives_and_survives_a_restart() {
    let mut env = Env::start();
    let store = env.store("wiki").await;
    let done = store.put(note("n", 8), "a person").await.unwrap();
    // The hub only hears of it later, so its own time differs.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    wiki.sync_once(store.as_ref()).await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let page = reader.pull(None, 0).await.unwrap();
    let a = &page.activities[0];
    let at = page.notes[&a.id].original_at.expect("originalAt arrives");
    assert_eq!(at.timestamp_millis(), done.at.timestamp_millis());
    assert!(
        a.at.timestamp_millis() >= at.timestamp_millis() + 50,
        "`at` is the hub's"
    );
    assert_eq!(a.actor, "wiki", "the program, never the person");

    drop((wiki, reader));
    env.hub.take().unwrap().stop().unwrap();
    env.hub = Some(Daemon::spawn(config(&env.dir)).unwrap());
    let reader = session(&env, notes_manifest("reader")).await;
    let page = reader.pull(None, 0).await.unwrap();
    let kept = page.notes[&page.activities[0].id].original_at;
    assert_eq!(
        kept.map(|t| t.timestamp_millis()),
        Some(done.at.timestamp_millis())
    );
}

#[tokio::test]
async fn original_actor_reaches_nobody_even_if_a_program_sends_it() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("raw").await;
            store.put(note_of_ada("n", 8), "a person").await.unwrap();
            let log = store.activities(&ActivityFilter::default()).await.unwrap();
            let writer = register(&env.socket(), &notes_manifest("raw")).await;
            assert_eq!(writer.push(&log, "ada@example.org").await.accepted, 1);

            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let answer = reader.pull(None, 0, false).await;
            let seen = &answer.activities[0];
            assert_eq!(seen.original_actor, "");
            assert_eq!(seen.actor, "raw");
            assert!(seen.original_at_ms.is_some());
            assert_eq!(seen.audience, Vec::<String>::new(), "the typed envelope");
            assert!(!seen.payload.contains("ada"), "{}", seen.payload);
            let held = env.hub().core().store();
            let log = held.activities(&ActivityFilter::default()).await.unwrap();
            assert!(
                !format!("{log:?}").contains("ada@example.org"),
                "not kept either"
            );
        })
        .await;
}

#[tokio::test]
async fn a_sensitive_object_says_neither_even_if_a_program_claims_them() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("raw").await;
            let mut plain = note("was plain", 8);
            plain.content["text"] = serde_json::json!("old secret");
            let mut marked = plain.clone();
            let id = plain.envelope.id;
            store.put(plain, "a person").await.unwrap();
            marked.envelope.sensitive = vec!["pd:Health".into()];
            store.put(marked, "a person").await.unwrap();
            let log = store.activities(&ActivityFilter::default()).await.unwrap();
            let writer = register(&env.socket(), &notes_manifest("raw")).await;

            // First the plain version; then the marked one, claiming a time and a person.
            writer.push(&log[..1], "").await;
            let outcome = writer
                .push_claiming(&log[1..], "ada@example.org", Some(1_800_000_000_000))
                .await;
            assert_eq!(outcome.withheld, 1);
            assert!(env.hub().core().store().get(id).await.unwrap().is_none());

            // What readers get is the removal of the old copy: it says no time and no person.
            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let answer = reader.pull(None, 0, false).await;
            let kinds: Vec<_> = answer.activities.iter().map(|a| a.kind.as_str()).collect();
            assert_eq!(kinds, ["Delete"], "nothing from before the mark");
            for entry in &answer.activities {
                assert_eq!(entry.original_at_ms, None, "{entry:?}");
                assert_eq!(entry.original_actor, "");
                assert_eq!(entry.payload_len, 0, "no old content: {}", entry.payload);
                assert_ne!(entry.name, "was plain", "not its old name either");
                assert_eq!((entry.at_ms, entry.actor.as_str()), (0, ""));
            }
        })
        .await;
}

#[tokio::test]
async fn deleting_what_was_sensitive_in_the_program_says_no_time() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("raw").await;
            let plain = note("was plain", 8);
            let mut marked = plain.clone();
            let id = plain.envelope.id;
            store.put(plain, "a person").await.unwrap();
            marked.envelope.sensitive = vec!["pd:Health".into()];
            store.put(marked, "a person").await.unwrap();
            store.delete(id, "a person").await.unwrap();
            let log = store.activities(&ActivityFilter::default()).await.unwrap();
            let writer = register(&env.socket(), &notes_manifest("raw")).await;

            // The hub holds the plain copy and never hears of the mark, only of the delete.
            writer.push(&log[..1], "").await;
            let outcome = writer
                .push_claiming(&log[2..], "", Some(1_800_000_000_000))
                .await;
            assert_eq!(outcome.accepted, 1);

            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let answer = reader.pull(None, 0, false).await;
            assert_eq!(answer.activities.len(), 1, "nothing from before the mark");
            assert_eq!(answer.activities[0].kind, "Delete");
            assert_eq!(answer.activities[0].original_at_ms, None);
        })
        .await;
}

/// Taking a mark off says no time either, whatever a program claims: the change comes from a state the
/// hub never saw.
#[tokio::test]
async fn a_change_out_of_a_sensitive_state_says_no_time() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("raw").await;
            let o = note("n", 8);
            let mut secret = o.clone();
            secret.envelope.sensitive = vec!["pd:Health".into()];
            store.put(secret, "a person").await.unwrap();
            store.put(o, "a person").await.unwrap();
            let log = store.activities(&ActivityFilter::default()).await.unwrap();
            let writer = register(&env.socket(), &notes_manifest("raw")).await;
            let outcome = writer
                .push_claiming(&log[1..], "", Some(1_800_000_000_000))
                .await;
            assert_eq!(outcome.accepted, 1);
            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let answer = reader.pull(None, 0, false).await;
            assert_eq!(answer.activities.len(), 1);
            assert_eq!(answer.activities[0].original_at_ms, None);
        })
        .await;
}

#[tokio::test]
async fn through_the_client_a_sensitive_object_is_withheld_with_no_time() {
    let env = Env::start();
    let store = env.store("wiki").await;
    let mut secret = note("secret", 8);
    secret.envelope.sensitive = vec!["pd:Health".into()];
    store.put(secret, "a person").await.unwrap();
    let wiki = session(&env, notes_manifest("wiki")).await;
    assert_eq!(wiki.sync_once(store.as_ref()).await.unwrap().withheld, 1);
    let reader = session(&env, notes_manifest("reader")).await;
    assert!(reader.pull(None, 0).await.unwrap().activities.is_empty());
}
