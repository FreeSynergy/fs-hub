//! Objects removed from the hub because their program marked them sensitive.
//!
//! The hub's log is append-only: the entries from before the mark still hold the old content, and the
//! delete's `before` image holds it too. Until `fs-store` has a purge, that content stays on the hub's
//! disk, and this list is what keeps it from going out: an entry at or before the removal is delivered
//! without payload and without time. A later version of the object, if the mark is taken off, is newer
//! and delivered as usual.
//!
//! Pattern: Repository — `MarkedRemovals` is the interface; `StoreMarked` keeps one small object per
//! removed object in the hub's `state.db`.

use std::sync::Arc;

use async_trait::async_trait;
use fs_store::traits::ObjectRepository;
use fs_store::{ActivityId, Envelope, ObjectId, StoredObject, TursoStore};
use serde::{Deserialize, Serialize};

use crate::Result;

const MARKED_TYPE: &str = "MarkedRemoval";
const MARKED_CONTENT_TYPE: &str = "application/json";
const MARKED_ACTOR: &str = "hub";

/// How far a removal reaches in the hub's log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reach {
    /// The removal is under way: everything logged so far, and its delete.
    Open,
    /// Everything up to and including this entry, the delete.
    UpTo(ActivityId),
}

impl Reach {
    fn covers(self, entry: ActivityId) -> bool {
        match self {
            Self::Open => true,
            Self::UpTo(last) => entry <= last,
        }
    }
}

/// The list of objects removed because they were marked.
#[async_trait]
pub trait MarkedRemovals: Send + Sync {
    /// Remember how far the removal of `object` reaches.
    async fn record(&self, object: ObjectId, reach: Reach) -> Result<()>;
    /// How far the removal of `object` reaches, if there was one.
    async fn find(&self, object: ObjectId) -> Result<Option<Reach>>;
    /// Is this log entry about `object` from before its removal?
    async fn covers(&self, object: ObjectId, entry: ActivityId) -> Result<bool> {
        Ok(self.find(object).await?.is_some_and(|r| r.covers(entry)))
    }
}

/// [`MarkedRemovals`] over a `TursoStore`.
pub struct StoreMarked {
    state: Arc<TursoStore>,
}

impl StoreMarked {
    /// Use `state`.
    pub fn new(state: Arc<TursoStore>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl MarkedRemovals for StoreMarked {
    async fn record(&self, object: ObjectId, reach: Reach) -> Result<()> {
        let mut envelope = Envelope::new(MARKED_TYPE, object.to_string());
        envelope.id = object;
        let content = serde_json::to_value(reach)?;
        let entry = StoredObject::new(envelope, MARKED_CONTENT_TYPE, content);
        self.state.put(entry, MARKED_ACTOR).await?;
        Ok(())
    }

    async fn find(&self, object: ObjectId) -> Result<Option<Reach>> {
        let Some(entry) = self.state.get(object).await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(entry.content)?))
    }
}
