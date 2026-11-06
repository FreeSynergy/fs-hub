//! When the programs did what the hub logged.
//!
//! The hub's log says when the hub applied a change, and the store sets that time itself. So the time
//! the program did it (`Activity.originalAt`) is kept beside the log, by the id of the hub's entry. A
//! sensitive or withheld object has no such entry, by construction: nothing is recorded for it. When
//! an object is marked later, its times are forgotten (`core/marked.rs`).
//!
//! Pattern: Repository — `OriginalTimes` is the interface; `StoreOriginals` keeps one small object per
//! entry in the hub's `state.db`, whose log `StoreCursors` keeps short.

use std::sync::Arc;

use async_trait::async_trait;
use fs_store::traits::ObjectRepository;
use fs_store::{ActivityId, Envelope, ObjectId, StoredObject, Timestamp, TursoStore};

use crate::Result;

const ORIGINAL_TYPE: &str = "OriginalTime";
const ORIGINAL_CONTENT_TYPE: &str = "application/json";
const ORIGINAL_ACTOR: &str = "hub";

/// The time a program did what a hub log entry records.
#[async_trait]
pub trait OriginalTimes: Send + Sync {
    /// Keep `at` for the hub's entry `logged`.
    async fn record(&self, logged: ActivityId, at: Timestamp) -> Result<()>;
    /// The time kept for the hub's entry `logged`.
    async fn find(&self, logged: ActivityId) -> Result<Option<Timestamp>>;
    /// Keep no time for the hub's entry `logged`.
    async fn forget(&self, logged: ActivityId) -> Result<()>;
}

/// [`OriginalTimes`] over a `TursoStore`.
pub struct StoreOriginals {
    state: Arc<TursoStore>,
}

impl StoreOriginals {
    /// Use `state`.
    pub fn new(state: Arc<TursoStore>) -> Self {
        Self { state }
    }
}

fn object_id(logged: ActivityId) -> Result<ObjectId> {
    // Both are UUIDv7.
    Ok(ObjectId::from_uuid(logged.as_uuid())?)
}

#[async_trait]
impl OriginalTimes for StoreOriginals {
    async fn record(&self, logged: ActivityId, at: Timestamp) -> Result<()> {
        let mut envelope = Envelope::new(ORIGINAL_TYPE, logged.to_string());
        envelope.id = object_id(logged)?;
        let content = serde_json::to_value(at)?;
        let object = StoredObject::new(envelope, ORIGINAL_CONTENT_TYPE, content);
        self.state.put(object, ORIGINAL_ACTOR).await?;
        Ok(())
    }

    async fn find(&self, logged: ActivityId) -> Result<Option<Timestamp>> {
        let Some(object) = self.state.get(object_id(logged)?).await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(object.content)?))
    }

    async fn forget(&self, logged: ActivityId) -> Result<()> {
        let id = object_id(logged)?;
        if self.state.get(id).await?.is_some() {
            self.state.delete(id, ORIGINAL_ACTOR).await?;
        }
        Ok(())
    }
}
