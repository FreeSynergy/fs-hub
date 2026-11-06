//! Taking a marked object off the hub, failing at each step: nothing old goes out at any point, and the
//! same push coming again finishes the work.

mod common;

use std::sync::atomic::Ordering;

use common::note;
use common::parts::{Parts, pushed};
use fs_hub::{ActivitySink, ActivitySource, Reach};
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{Activity, ActivityFilter, ActivityKind, OpenOptions, StoredObject, TursoStore};

const OLD: &str = "old secret text";

struct Case {
    parts: Parts,
    object: StoredObject,
    /// The plain entry, then the entry that marks it.
    log: Vec<Activity>,
    _dir: tempfile::TempDir,
}

async fn case() -> Case {
    let dir = tempfile::tempdir().unwrap();
    let parts = Parts::open(dir.path()).await;
    let options = OpenOptions {
        allow_sensitive_unencrypted: true,
        ..Default::default()
    };
    let source = TursoStore::open_with(dir.path().join("source.db"), options)
        .await
        .unwrap();
    let mut object = note("real name", 8);
    object.content["text"] = serde_json::json!(OLD);
    let mut marked = object.clone();
    marked.envelope.sensitive = vec!["pd:Health".into()];
    source.put(object.clone(), "owner").await.unwrap();
    source.put(marked, "owner").await.unwrap();
    let log = source.activities(&ActivityFilter::default()).await.unwrap();
    parts
        .collector
        .push("owner", pushed(&log[..1]))
        .await
        .unwrap();
    Case {
        parts,
        object,
        log,
        _dir: dir,
    }
}

impl Case {
    async fn mark(&self) -> fs_hub::Result<fs_hub_proto::PushOutcome> {
        self.parts
            .collector
            .push("owner", pushed(&self.log[1..]))
            .await
    }
    async fn reach(&self) -> Option<Reach> {
        let found = self
            .parts
            .marked
            .find(self.object.envelope.id)
            .await
            .unwrap();
        found.map(|r| r.reach)
    }
    /// What a late reader gets now; nothing of the old may be in it.
    async fn kinds_a_late_reader_gets(&self) -> Vec<ActivityKind> {
        let page = self
            .parts
            .source
            .pull("late", None, 0, false)
            .await
            .unwrap();
        let text = format!("{:?}{:?}", page.activities, page.notes);
        assert!(!text.contains(OLD) && !text.contains("real name"), "{text}");
        page.activities.iter().map(|a| a.kind).collect()
    }
}

#[tokio::test]
async fn the_delete_fails() {
    let c = case().await;
    c.parts.objects.inject_fault_before_activity(true);
    assert!(c.mark().await.is_err());
    assert_eq!(c.reach().await, Some(Reach::Open), "the list came first");
    assert!(
        c.parts
            .objects
            .get(c.object.envelope.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(c.kinds_a_late_reader_gets().await, []);

    c.mark().await.unwrap();
    assert!(matches!(c.reach().await, Some(Reach::UpTo(_))));
    assert_eq!(c.kinds_a_late_reader_gets().await, [ActivityKind::Delete]);
}

#[tokio::test]
async fn closing_the_list_fails_after_the_delete() {
    let c = case().await;
    c.parts.faults.closing.store(true, Ordering::SeqCst);
    assert!(c.mark().await.is_err());
    assert_eq!(c.reach().await, Some(Reach::Open));
    assert!(
        c.parts
            .objects
            .get(c.object.envelope.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(c.kinds_a_late_reader_gets().await, [ActivityKind::Delete]);

    // The same push comes again and finds no copy: it finishes the work.
    c.mark().await.unwrap();
    let hub_log = c
        .parts
        .objects
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(
        c.reach().await,
        Some(Reach::UpTo(hub_log.last().unwrap().id))
    );
}

#[tokio::test]
async fn forgetting_the_times_fails_before_the_list_closes() {
    let c = case().await;
    c.parts.faults.forgetting.store(true, Ordering::SeqCst);
    assert!(c.mark().await.is_err());
    assert_eq!(
        c.reach().await,
        Some(Reach::Open),
        "times first, then closing"
    );
    assert_eq!(c.kinds_a_late_reader_gets().await, [ActivityKind::Delete]);
    c.mark().await.unwrap();
    assert!(matches!(c.reach().await, Some(Reach::UpTo(_))));
}

#[tokio::test]
async fn the_times_are_gone_without_a_trace() {
    let c = case().await;
    let hub_log = c
        .parts
        .objects
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    let plain_entry = hub_log[0].id;
    assert!(c.parts.originals.find(plain_entry).await.unwrap().is_some());
    c.mark().await.unwrap();
    assert_eq!(c.parts.originals.find(plain_entry).await.unwrap(), None);
    c.parts.originals.settle().await.unwrap();
    let times_log = c
        .parts
        .times_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    assert_eq!(times_log.len(), 0, "{times_log:?}");
}

/// Whatever is on file: the delete that goes out says no time.
#[tokio::test]
async fn a_covered_delete_says_no_time_and_nobody() {
    let c = case().await;
    c.mark().await.unwrap();
    let hub_log = c
        .parts
        .objects
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    let delete = hub_log.last().unwrap();
    c.parts
        .originals
        .record(delete.id, chrono::Utc::now())
        .await
        .unwrap();
    let page = c.parts.source.pull("late", None, 0, false).await.unwrap();
    let a = &page.activities[0];
    assert_eq!(a.kind, ActivityKind::Delete);
    assert_eq!((a.at.timestamp_millis(), a.actor.as_str()), (0, ""));
    assert!(a.payload.is_null());
    assert_eq!(page.notes[&a.id].original_at, None);
}
