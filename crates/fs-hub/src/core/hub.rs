//! The hub core: wiring of registry, sink, source and proposal engine.
//!
//! Pattern: Facade — one entry point for the RPC layer and tests; every capability behind it is a
//! trait object (`ProgramRegistry`, `ActivitySink`, `ActivitySource`, `ProposalEngine`), so each
//! can be replaced or tested alone. No domain logic (ADR 0019).

use std::path::Path;
use std::sync::Arc;

use fs_hub_proto::{Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome, Pushed};
use fs_store::traits::{ActivityLog, ChangeFeed, ObjectQuery, ObjectRepository};
use fs_store::{Activity, ActivityId, Store, TursoStore};
use tokio::sync::broadcast;

use super::activity::{ActivitySink, ActivitySource};
use super::collector::ObjectCollector;
use super::cursors::StoreCursors;
use super::log_source::LogSource;
use super::marked::{MarkedRemovals, StoreMarked};
use super::originals::{OriginalTimes, StoreOriginals};
use super::presence::LiveGuard;
use super::proposals::{ProposalEngine, SharedIdentity};
use super::registry::{ProgramRegistry, StoredRegistry};
use super::visibility::Visibility;
use crate::Result;

/// The hub: its own object store (everything pushed) plus a small state store for cursors.
pub struct HubCore {
    objects: Arc<TursoStore>,
    registry: Arc<dyn ProgramRegistry>,
    sink: Arc<dyn ActivitySink>,
    source: Arc<dyn ActivitySource>,
    engine: Arc<dyn ProposalEngine>,
}

impl HubCore {
    /// Open (or create) the hub's databases in `dir`: `objects.db` and `state.db`.
    pub async fn open(dir: &Path) -> Result<Arc<Self>> {
        Self::open_with(dir, Arc::new(SharedIdentity)).await
    }

    /// Like [`HubCore::open`] with another proposal strategy.
    pub async fn open_with(dir: &Path, engine: Arc<dyn ProposalEngine>) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir)?;
        let objects = Arc::new(TursoStore::open(dir.join("objects.db")).await?);
        let state = Arc::new(TursoStore::open(dir.join("state.db")).await?);
        let originals: Arc<dyn OriginalTimes> = Arc::new(StoreOriginals::new(state.clone()));
        let marked: Arc<dyn MarkedRemovals> = Arc::new(StoreMarked::new(state.clone()));
        let registry: Arc<dyn ProgramRegistry> =
            Arc::new(StoredRegistry::open(Arc::new(StoreCursors::new(state).await?)).await?);
        let repo: Arc<dyn ObjectRepository> = objects.clone();
        let log: Arc<dyn ActivityLog> = objects.clone();
        let feed: Arc<dyn ChangeFeed> = objects.clone();
        Ok(Arc::new(Self {
            sink: Arc::new(ObjectCollector::new(
                registry.clone(),
                repo.clone(),
                log.clone(),
                originals.clone(),
                marked.clone(),
            )),
            source: Arc::new(LogSource::new(
                registry.clone(),
                log,
                feed,
                Visibility::new(repo),
                originals,
                marked,
            )),
            registry,
            engine,
            objects,
        }))
    }

    /// The hub's own store (read access for tests and tools; the hub is its only writer).
    pub fn store(&self) -> Arc<dyn Store> {
        self.objects.clone()
    }

    /// A program announces itself. Returns the guard that counts the session as live.
    pub async fn register(&self, manifest: Manifest) -> Result<LiveGuard> {
        self.registry.register(manifest).await
    }

    /// Manifest of a registered program.
    pub async fn manifest(&self, program: &str) -> Result<Manifest> {
        self.registry.manifest(program).await
    }

    /// All programs the hub knows (connected or not).
    pub async fn programs(&self) -> Vec<ProgramInfo> {
        self.registry.programs().await
    }

    /// Apply a program's activities (see [`ActivitySink::push`]).
    pub async fn push(&self, program: &str, activities: Vec<Pushed>) -> Result<PushOutcome> {
        self.sink.push(program, activities).await
    }

    /// Other programs' activities (see [`ActivitySource::pull`]).
    pub async fn pull(
        &self,
        program: &str,
        since: Option<ActivityId>,
        limit: usize,
        envelope_only: bool,
    ) -> Result<PullBatch> {
        self.source.pull(program, since, limit, envelope_only).await
    }

    /// Acknowledge delivery (see [`ActivitySource::ack`]).
    pub async fn ack(&self, program: &str, up_to: ActivityId) -> Result<()> {
        self.source.ack(program, up_to).await
    }

    /// Would `program` (with these `reads`) get this activity?
    pub async fn visible_to(&self, program: &str, reads: &[String], a: &Activity) -> Result<bool> {
        self.source.visible_to(program, reads, a).await
    }

    /// Live feed of the hub's activities (for subscriptions).
    pub fn changes(&self) -> broadcast::Receiver<Activity> {
        self.source.changes()
    }

    /// Same-person candidates across programs (see [`ProposalEngine`]).
    pub async fn proposals(&self) -> Result<Vec<Proposal>> {
        let query: &dyn ObjectQuery = self.objects.as_ref();
        let log: &dyn ActivityLog = self.objects.as_ref();
        self.engine.propose(query, log).await
    }
}
