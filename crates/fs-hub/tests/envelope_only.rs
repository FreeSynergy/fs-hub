//! End-to-end: `pull(..., envelopeOnly)` really carries no payload, and leaves `sync_once` alone.

mod common;

use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_objects::ActivityKind;
use fs_objects::traits::{ActivityLog, ObjectRepository};

const MIB: usize = 1 << 20;
/// More than one page, if the content came along.
const NOTES: usize = 40;

#[tokio::test]
async fn the_wire_holds_no_payload_and_the_page_is_not_cut() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("wiki").await;
            let wiki = session(&env, notes_manifest("wiki")).await;
            for i in 0..NOTES {
                store
                    .put(note(&format!("N{i}"), MIB), "wiki")
                    .await
                    .unwrap();
                if i % 8 == 7 {
                    wiki.sync_once(store.as_ref()).await.unwrap();
                }
            }
            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let answer = raw.pull(None, 0, true).await;
            assert_eq!(answer.activities.len(), NOTES, "no content, so one page");
            assert!(!answer.more);
            for (i, a) in answer.activities.iter().enumerate() {
                assert_eq!(a.payload_len, 0, "{a:?}");
                assert_eq!(a.object_type, "Note");
                assert_eq!(a.name, format!("N{i}"));
            }
            // The same pull with content is cut, and does have it.
            let full = raw.pull(None, 0, false).await;
            assert!(full.more && full.activities.len() < NOTES);
            assert!(full.activities[0].payload_len > MIB);
        })
        .await;
}

#[tokio::test]
async fn the_session_gets_summaries_and_sync_once_still_gets_payloads() {
    let env = Env::start();
    let store = env.store("wiki").await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    let o = note("Ada's note", 64);
    let id = o.envelope.id;
    store.put(o, "wiki").await.unwrap();
    store
        .log(
            ActivityKind::Read,
            id,
            "wiki",
            serde_json::json!({"by": "x"}),
        )
        .await
        .unwrap();
    assert_eq!(wiki.sync_once(store.as_ref()).await.unwrap().accepted, 2);

    let reader = session(&env, notes_manifest("reader")).await;
    let page = reader.pull_envelopes(None, 0).await.unwrap();
    assert_eq!(page.entries.len(), 2);
    for entry in &page.entries {
        assert_eq!(
            entry.envelope.id, id,
            "log-only entries say what they are about"
        );
        assert_eq!(entry.envelope.name, "Ada's note");
    }

    // It moved nothing: a sync still gets all of it, with the content.
    let own = env.store("reader").await;
    let report = reader.sync_once(own.as_ref()).await.unwrap();
    assert_eq!(
        (report.pulled, report.failed.len()),
        (2, 0),
        "{:?}",
        report.failed
    );
    let got = own.get(id).await.unwrap().unwrap();
    assert_eq!(got.content["text"].as_str().unwrap().len(), 64);
}

/// The place of a summary reader is its own, and `sync_once` is not affected by it.
#[tokio::test]
async fn summaries_page_with_their_own_cursor_and_the_hubs_stays() {
    let env = Env::start();
    let store = env.store("wiki").await;
    let wiki = session(&env, notes_manifest("wiki")).await;
    for i in 0..5 {
        store.put(note(&format!("N{i}"), 8), "wiki").await.unwrap();
    }
    wiki.sync_once(store.as_ref()).await.unwrap();

    let reader = session(&env, notes_manifest("reader")).await;
    let (mut names, mut place) = (Vec::new(), None);
    loop {
        let page = reader.pull_envelopes(place, 2).await.unwrap();
        names.extend(page.entries.iter().map(|e| e.envelope.name.clone()));
        place = page.next;
        if !page.more {
            break;
        }
    }
    assert_eq!(names, ["N0", "N1", "N2", "N3", "N4"]);
    // A place survives as text.
    let text = place.unwrap().to_string();
    assert_eq!(
        text.parse::<fs_hub_client::EnvelopeCursor>().unwrap(),
        place.unwrap()
    );

    let own = env.store("reader").await;
    let report = reader.sync_once(own.as_ref()).await.unwrap();
    assert_eq!(report.applied, 5, "the content still comes");
}
