//! Persistence of the per-program cursors.
//!
//! Pattern: Repository — `CursorRepository` is the interface (load all, save one); `StoreCursors`
//! keeps the states as objects in the hub's small `state.db` (an `fs-objects` `TursoStore`).

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use async_trait::async_trait;
use fs_objects::traits::{HistoryRetention, ObjectQuery, ObjectRepository};
use fs_objects::{Envelope, Query, Retention, StoredObject, TursoStore};

use super::state::{ProgramState, now_ms};
use crate::Result;

const STATE_TYPE: &str = "ProgramState";
const STATE_CONTENT_TYPE: &str = "application/json";
const COMPACT_EVERY_MS: i64 = 1000;

/// Where program states are kept.
#[async_trait]
pub trait CursorRepository: Send + Sync {
    /// Every stored state (called once at startup).
    async fn load_all(&self) -> Result<Vec<ProgramState>>;
    /// Create or update one program's state.
    async fn save(&self, state: &ProgramState) -> Result<()>;
}

/// [`CursorRepository`] over a `TursoStore`.
pub struct StoreCursors {
    state: Arc<TursoStore>,
    compacted_ms: AtomicI64,
}

impl StoreCursors {
    /// Use `state` (its history is kept short: the store has no sync peer).
    pub async fn new(state: Arc<TursoStore>) -> Result<Self> {
        state.set_retention(Retention::steps(20)).await?;
        Ok(Self {
            state,
            compacted_ms: AtomicI64::new(0),
        })
    }
}

#[async_trait]
impl CursorRepository for StoreCursors {
    async fn load_all(&self) -> Result<Vec<ProgramState>> {
        self.state
            .query(&Query::new().of_type(STATE_TYPE))
            .await?
            .into_iter()
            .map(|o| Ok(serde_json::from_value(o.content)?))
            .collect()
    }

    async fn save(&self, st: &ProgramState) -> Result<()> {
        let mut env = Envelope::new(STATE_TYPE, st.manifest.program.clone());
        env.id = st.object_id;
        let obj = StoredObject::new(env, STATE_CONTENT_TYPE, serde_json::to_value(st)?)
            .with_index("program", st.manifest.program.clone());
        let act = self.state.put(obj, "hub").await?;
        // The state store has no peer; keep its log small.
        self.state.mark_synced(act.id).await?;
        // A save comes before every answer to a push; compacting that often is what is slow.
        let now = now_ms();
        if now - self.compacted_ms.load(Ordering::Relaxed) >= COMPACT_EVERY_MS {
            self.compacted_ms.store(now, Ordering::Relaxed);
            self.state.compact().await?;
        }
        Ok(())
    }
}
