//! `purge_object` in a real program store (ADR 0029): later writes are logged without state. The hub
//! keeps its copy through such an entry, the reader deletes nothing, the live feed tells no deletion.
//! A copy goes only for a reason of its own: the object is marked, or it is really gone.
//!
//! Pattern: Scenario test — one story per test, told through the real servers, the hub and the client.

mod common;

use common::story::{Story, deletes, last_entry_is_redacted};
use common::{held, mark_sensitive};
use fs_hub_client::ChangeNotice;
use fs_store::ActivityKind;

#[tokio::test]
async fn a_change_after_a_purge_keeps_the_copy_and_tells_no_deletion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut s = Story::begin().await;
            s.purge().await;
            s.p.change(s.ada, "u1", "Ada Lovelace", "ada@example.org", "+49 30 1")
                .await;
            assert_eq!(
                last_entry_is_redacted(&s.p.store, s.ada).await,
                ActivityKind::Update
            );
            s.settle().await;
            // The hub took the object's current copy in place of the entry without state.
            assert_eq!(
                held(&s.c.store, s.ada).await.unwrap().envelope.name,
                "Ada Lovelace"
            );
            assert!(held(&s.p.store, s.ada).await.is_some());
            let told = s.told().await;
            assert_eq!(deletes(&told, s.ada), 0, "{told:?}");
            assert!(
                told.iter()
                    .any(|n| matches!(n, ChangeNotice::Updated { .. })),
                "{told:?}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_mark_after_a_purge_takes_the_copy_off_the_hub_for_the_mark() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut s = Story::begin().await;
            s.purge().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            assert_eq!(
                last_entry_is_redacted(&s.p.store, s.ada).await,
                ActivityKind::Update
            );
            s.settle().await;
            // The reason is the mark (the hub reads it from the current copy), not the entry.
            assert!(
                held(&s.c.store, s.ada).await.is_none(),
                "taken off the hub, copies go"
            );
            assert!(
                held(&s.p.store, s.ada).await.is_some(),
                "the owner keeps it"
            );
            let _ = s.told().await;
        })
        .await;
}

#[tokio::test]
async fn a_real_delete_after_a_purge_still_removes_the_copy() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut s = Story::begin().await;
            s.purge().await;
            s.p.delete(s.ada).await;
            assert_eq!(
                last_entry_is_redacted(&s.p.store, s.ada).await,
                ActivityKind::Delete
            );
            s.settle().await;
            // The store no longer has the object: the client says so, the entry itself never does.
            assert!(held(&s.c.store, s.ada).await.is_none());
            assert_eq!(deletes(&s.told().await, s.ada), 1);
        })
        .await;
}
