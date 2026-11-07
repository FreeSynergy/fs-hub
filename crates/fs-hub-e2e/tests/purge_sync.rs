//! Precondition of `purge_object`: sync first. A purge deletes unsynced log entries too, so a mark that
//! the hub has not heard of would be lost and the hub would keep the unmarked copy (ADR 0029).
//! `sync_then_purge` of the hub client is the way for programs.
//!
//! Pattern: Scenario test — one story per test, told through the real servers, the hub and the client.

mod common;

use common::story::Story;
use common::{held, id, mark_sensitive};
use fs_objects::ActivityFilter;
use fs_objects::traits::ActivityLog;

async fn entries_of(s: &Story) -> usize {
    let filter = ActivityFilter {
        object: Some(id(s.ada)),
        ..Default::default()
    };
    s.p.store.activities(&filter).await.unwrap().len()
}

#[tokio::test]
async fn a_direct_purge_before_the_sync_loses_the_mark_and_says_so() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            let report = s.purge().await;
            assert!(report.unsynced_removed >= 1, "the signal: {report:?}");
            s.settle().await;
            assert!(
                held(&s.c.store, s.ada).await.is_some(),
                "the hub never heard of the mark: the copy stays"
            );
        })
        .await;
}

#[tokio::test]
async fn sync_then_purge_takes_the_mark_to_the_hub_first() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            let report =
                s.p.session
                    .sync_then_purge(s.p.store.as_ref(), id(s.ada))
                    .await
                    .unwrap();
            assert_eq!(report.unsynced_removed, 0, "{report:?}");
            assert!(report.removed > 0);
            s.settle().await;
            assert!(held(&s.c.store, s.ada).await.is_none(), "the mark arrived");
            assert!(
                held(&s.p.store, s.ada).await.is_some(),
                "the owner keeps it"
            );
            assert_eq!(entries_of(&s).await, 0, "the history is gone");
        })
        .await;
}

#[tokio::test]
async fn without_a_reachable_hub_nothing_is_purged() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            mark_sensitive(&s.p.store, s.ada, "me").await;
            let before = entries_of(&s).await;
            let Story { env, p, ada, .. } = s;
            env.hub.stop().unwrap();
            let answer = p.session.sync_then_purge(p.store.as_ref(), id(ada)).await;
            assert!(answer.is_err(), "fail closed");
            let filter = ActivityFilter {
                object: Some(id(ada)),
                ..Default::default()
            };
            assert_eq!(p.store.activities(&filter).await.unwrap().len(), before);
        })
        .await;
}
