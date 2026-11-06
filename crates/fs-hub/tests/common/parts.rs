//! The hub's core parts over real stores, without a socket: to fail at one exact step.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use fs_hub::{
    Error, LogSource, MarkedRemovals, ObjectCollector, OriginalTimes, ProgramRegistry, Reach,
    Removal, StoreCursors, StoreMarked, StoreOriginals, StoredRegistry, Visibility,
};
use fs_hub_proto::Pushed;
use fs_store::{Activity, ActivityId, ObjectId, Timestamp, TursoStore};

use super::notes_manifest;

/// Fails once when asked to close a removal.
#[derive(Default)]
pub struct Faults {
    pub closing: AtomicBool,
    pub forgetting: AtomicBool,
}

fn injected() -> Error {
    Error::Store(fs_store::Error::Backend("injected".into()))
}

struct FaultyMarks(Arc<dyn MarkedRemovals>, Arc<Faults>);

#[async_trait]
impl MarkedRemovals for FaultyMarks {
    async fn record(&self, object: ObjectId, removal: &Removal) -> fs_hub::Result<()> {
        if removal.reach != Reach::Open && self.1.closing.swap(false, Ordering::SeqCst) {
            return Err(injected());
        }
        self.0.record(object, removal).await
    }
    async fn find(&self, object: ObjectId) -> fs_hub::Result<Option<Removal>> {
        self.0.find(object).await
    }
    async fn all(&self) -> fs_hub::Result<Vec<(ObjectId, Removal)>> {
        self.0.all().await
    }
}

struct FaultyTimes(Arc<dyn OriginalTimes>, Arc<Faults>);

#[async_trait]
impl OriginalTimes for FaultyTimes {
    async fn record(&self, logged: ActivityId, at: Timestamp) -> fs_hub::Result<()> {
        self.0.record(logged, at).await
    }
    async fn find(&self, logged: ActivityId) -> fs_hub::Result<Option<Timestamp>> {
        self.0.find(logged).await
    }
    async fn forget(&self, logged: ActivityId) -> fs_hub::Result<()> {
        if self.1.forgetting.swap(false, Ordering::SeqCst) {
            return Err(injected());
        }
        self.0.forget(logged).await
    }
    async fn settle(&self) -> fs_hub::Result<()> {
        self.0.settle().await
    }
}

pub struct Parts {
    pub objects: Arc<TursoStore>,
    pub times_store: Arc<TursoStore>,
    pub marks_store: Arc<TursoStore>,
    pub collector: ObjectCollector,
    pub source: LogSource,
    pub originals: Arc<dyn OriginalTimes>,
    pub marked: Arc<dyn MarkedRemovals>,
    pub faults: Arc<Faults>,
    _live: Vec<fs_hub::LiveGuard>,
}

impl Parts {
    /// Registers `owner` and `late`, both may read and write notes.
    pub async fn open(dir: &Path) -> Self {
        let open = |name: &str| TursoStore::open(dir.join(name));
        let objects = Arc::new(open("objects.db").await.unwrap());
        let state = Arc::new(open("state.db").await.unwrap());
        let times_store = Arc::new(open("times.db").await.unwrap());
        let marks_store = Arc::new(open("marks.db").await.unwrap());
        let faults = Arc::new(Faults::default());
        let registry: Arc<dyn ProgramRegistry> = Arc::new(
            StoredRegistry::open(Arc::new(StoreCursors::new(state).await.unwrap()))
                .await
                .unwrap(),
        );
        let mut live = Vec::new();
        for name in ["owner", "late"] {
            live.push(registry.register(notes_manifest(name)).await.unwrap());
        }
        let originals: Arc<dyn OriginalTimes> = Arc::new(FaultyTimes(
            Arc::new(StoreOriginals::new(times_store.clone()).await.unwrap()),
            faults.clone(),
        ));
        let marked: Arc<dyn MarkedRemovals> = Arc::new(FaultyMarks(
            Arc::new(StoreMarked::new(marks_store.clone()).await.unwrap()),
            faults.clone(),
        ));
        Self {
            collector: ObjectCollector::new(
                registry.clone(),
                objects.clone(),
                objects.clone(),
                originals.clone(),
                marked.clone(),
            ),
            source: LogSource::new(
                registry,
                objects.clone(),
                objects.clone(),
                Visibility::new(objects.clone()),
                originals.clone(),
                marked.clone(),
            ),
            objects,
            times_store,
            marks_store,
            originals,
            marked,
            faults,
            _live: live,
        }
    }
}

/// `log` as a program hands it over, each with the time it was written.
pub fn pushed(log: &[Activity]) -> Vec<Pushed> {
    log.iter()
        .map(|a| Pushed {
            activity: a.clone(),
            original_at: Some(a.at),
        })
        .collect()
}
