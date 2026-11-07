//! A card shared between the people and the calendar program, to tell purge stories on.
//!
//! Pattern: Facade (test fixture) — one `Story` hides the two programs, the hub and the live feed.
#![allow(dead_code)]

use std::time::Duration;

use super::{Env, Prog, calendar, held, id, people, settle};
use fs_hub_client::ChangeNotice;
use fs_interfaces::events_capnp::items;
use fs_interfaces::people_capnp::persons;
use fs_objects::traits::{ActivityLog, HistoryRetention};
use fs_objects::{ActivityFilter, ActivityKind};
use tokio::sync::mpsc::UnboundedReceiver;
use uuid::Uuid;

pub type Feed = UnboundedReceiver<ChangeNotice>;

/// Everything the feed has told by now (it is quiet for a moment once the hub is done).
pub async fn told(feed: &mut Feed) -> Vec<ChangeNotice> {
    let mut all = Vec::new();
    while let Ok(Some(n)) = tokio::time::timeout(Duration::from_millis(700), feed.recv()).await {
        all.push(n);
    }
    all
}

pub fn deletes(notices: &[ChangeNotice], object: Uuid) -> usize {
    notices
        .iter()
        .filter(|n| matches!(n, ChangeNotice::Deleted { id: d, .. } if d.as_uuid() == object))
        .count()
}

/// The newest log entry of `object` in `store` is one without state.
pub async fn last_entry_is_redacted(store: &fs_objects::TursoStore, object: Uuid) -> ActivityKind {
    let filter = ActivityFilter {
        object: Some(id(object)),
        ..Default::default()
    };
    let last = store.activities(&filter).await.unwrap().pop().unwrap();
    assert!(last.is_redacted(), "the write after a purge has no images");
    last.kind
}

/// A card of the people program, shared with the calendar (which listens).
pub struct Story {
    pub env: Env,
    pub p: Prog<persons::Client>,
    pub c: Prog<items::Client>,
    pub feed: Feed,
    pub ada: Uuid,
}

impl Story {
    pub async fn told(&mut self) -> Vec<ChangeNotice> {
        told(&mut self.feed).await
    }

    pub async fn settle(&self) {
        settle(&[
            (&self.p.session, self.p.store.as_ref()),
            (&self.c.session, self.c.store.as_ref()),
        ])
        .await;
    }

    pub async fn begin() -> Self {
        let env = Env::start();
        let p = people::start(&env, "people").await;
        let c = calendar::start(&env, "calendar").await;
        let feed = c.session.subscribe().await.unwrap();
        let ada = p.create("u1", "Ada", "ada@example.org", "+49 30 1").await;
        let mut story = Self {
            env,
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
        story
    }

    /// The people program purges the card's history (ADR 0029).
    pub async fn purge(&self) -> fs_objects::PurgeReport {
        let report = self.p.store.purge_object(id(self.ada)).await.unwrap();
        assert!(report.removed > 0);
        report
    }
}
