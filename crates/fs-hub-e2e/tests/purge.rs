//! `purge_object` in a real program store (ADR 0029): later writes are logged without state. The hub
//! keeps its copy through such an entry, the reader deletes nothing, the live feed tells no deletion.
//! A copy goes only for a reason of its own: the object is marked, or it is really gone.
//!
//! Pattern: Scenario test — one story per test, told through the real servers, the hub and the client.

mod common;

use std::time::Duration;

use common::{Env, Prog, calendar, held, id, mark_sensitive, people, settle};
use fs_hub_client::ChangeNotice;
use fs_interfaces::events_capnp::items;
use fs_interfaces::people_capnp::persons;
use fs_store::traits::{ActivityLog, HistoryRetention};
use fs_store::{ActivityFilter, ActivityKind};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

type Feed = UnboundedReceiver<ChangeNotice>;

/// Everything the feed has told by now (it is quiet for a moment once the hub is done).
async fn told(feed: &mut Feed) -> Vec<ChangeNotice> {
    let mut all = Vec::new();
    while let Ok(Some(n)) = tokio::time::timeout(Duration::from_millis(700), feed.recv()).await {
        all.push(n);
    }
    all
}

fn deletes(notices: &[ChangeNotice], object: Uuid) -> usize {
    notices
        .iter()
        .filter(|n| matches!(n, ChangeNotice::Deleted { id: d, .. } if d.as_uuid() == object))
        .count()
}

/// The newest log entry of `object` in `store` is one without state.
async fn last_entry_is_redacted(store: &fs_store::TursoStore, object: Uuid) -> ActivityKind {
    let filter = ActivityFilter {
        object: Some(id(object)),
        ..Default::default()
    };
    let last = store.activities(&filter).await.unwrap().pop().unwrap();
    assert!(last.is_redacted(), "the write after a purge has no images");
    last.kind
}

/// A card of the people program, shared with the calendar (which listens), then purged in its store.
struct Story {
    _env: Env,
    p: Prog<persons::Client>,
    c: Prog<items::Client>,
    feed: Feed,
    ada: Uuid,
}

impl Story {
    async fn told(&mut self) -> Vec<ChangeNotice> {
        told(&mut self.feed).await
    }

    async fn settle(&self) {
        settle(&[
            (&self.p.session, self.p.store.as_ref()),
            (&self.c.session, self.c.store.as_ref()),
        ])
        .await;
    }

    async fn begin() -> Self {
        let env = Env::start();
        let p = people::start(&env, "people").await;
        let c = calendar::start(&env, "calendar").await;
        let feed = c.session.subscribe().await.unwrap();
        let ada = p.create("u1", "Ada", "ada@example.org", "+49 30 1").await;
        let mut story = Self {
            _env: env,
            p,
            c,
            feed,
            ada,
        };
        story.settle().await;
        assert!(held(&story.c.store, ada).await.is_some(), "shared first");
        let first = story.told().await;
        assert!(
            matches!(first.as_slice(), [ChangeNotice::Created { .. }]),
            "{first:?}"
        );
        let report = story.p.store.purge_object(id(ada)).await.unwrap();
        assert!(report.removed > 0);
        story
    }
}

#[tokio::test]
async fn a_change_after_a_purge_keeps_the_copy_and_tells_no_deletion() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut s = Story::begin().await;
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
