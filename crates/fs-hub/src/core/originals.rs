//! When the programs did what the hub logged.
//!
//! The hub's log says when the hub applied a change, and the store sets that time itself. So the time
//! the program did it (`Activity.originalAt`) is kept beside the log, by the id of the hub's entry. A
//! sensitive or withheld object has no such entry, by construction: nothing is recorded for it. When
//! an object is marked later, its times are forgotten (`core/marked.rs`), in the store's log as well.
//!
//! Pattern: Repository — `OriginalTimes` is the interface; `StoreOriginals` keeps one small object per
//! entry in the hub's `times.db`, a store of its own: its ids are nobody else's.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_objects::traits::{HistoryRetention, ObjectRepository};
use fs_objects::{ActivityId, Envelope, ObjectId, Retention, StoredObject, Timestamp, TursoStore};

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
    /// Keep no time for the hub's entry `logged`, and no trace of it.
    async fn forget(&self, logged: ActivityId) -> Result<()>;
    /// Keep the store's own log short; not on every record, that would be slow.
    async fn settle(&self) -> Result<()>;
}

/// [`OriginalTimes`] over a `TursoStore`.
pub struct StoreOriginals {
    times: Arc<TursoStore>,
    unsettled: Mutex<Option<ActivityId>>,
}

impl StoreOriginals {
    /// Use `times`, a store for this alone: it keeps no history.
    pub async fn new(times: Arc<TursoStore>) -> Result<Self> {
        times.set_retention(Retention::steps(0)).await?;
        Ok(Self {
            times,
            unsettled: Mutex::new(None),
        })
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
        let written = self.times.put(object, ORIGINAL_ACTOR).await?;
        *self.unsettled.lock().unwrap() = Some(written.id);
        Ok(())
    }

    async fn find(&self, logged: ActivityId) -> Result<Option<Timestamp>> {
        let found = self.times.get(object_id(logged)?).await?;
        let Some(object) = found.filter(|o| o.envelope.object_type == ORIGINAL_TYPE) else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(object.content)?))
    }

    async fn forget(&self, logged: ActivityId) -> Result<()> {
        let id = object_id(logged)?;
        if self.times.get(id).await?.is_some() {
            let written = self.times.delete(id, ORIGINAL_ACTOR).await?;
            *self.unsettled.lock().unwrap() = Some(written.id);
            self.settle().await?;
        }
        Ok(())
    }

    async fn settle(&self) -> Result<()> {
        let unsettled = self.unsettled.lock().unwrap().take();
        if let Some(last) = unsettled {
            self.times.mark_synced(last).await?;
            self.times.compact().await?;
        }
        Ok(())
    }
}
