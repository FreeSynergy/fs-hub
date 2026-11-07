//! Objects taken off the hub because a program marked them sensitive.
//!
//! The hub's log is append-only: the entries from before the mark still hold the old content, and the
//! delete's `before` image holds it too. Until `fs-objects` has a purge, that content stays on the hub's
//! disk, and this list is all that keeps it from going out. So `marks.db` is safety-relevant: reset it,
//! and the old content can be delivered again.
//!
//! An entry within the reach of a removal goes to nobody, except the delete, which readers get as a
//! plain delete. The program that first wrote the object does not get that either: a mark takes the
//! object off the hub, it never deletes another program's object. The owner and the program that set
//! the mark can lift it, nobody else, and the owner goes on writing while it holds.
//!
//! Pattern: Repository — `MarkedRemovals` is the interface; `StoreMarked` keeps one small object per
//! removed object in a store of its own: a program chooses object ids, and this way they meet nothing
//! but its own entry.

use std::sync::Arc;

use async_trait::async_trait;
use fs_objects::traits::{HistoryRetention, ObjectQuery, ObjectRepository};
use fs_objects::{ActivityId, Envelope, ObjectId, Query, Retention, StoredObject, TursoStore};
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
    /// Everything up to and including this entry.
    UpTo(ActivityId),
}

/// One object on the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removal {
    /// How far it reaches.
    pub reach: Reach,
    /// The program that set the mark.
    pub by: String,
    /// The program that first wrote the object.
    pub origin: String,
    /// The mark still holds: only `by` can bring the object back.
    pub holds: bool,
}

impl Removal {
    /// Is this log entry from before the removal?
    pub fn covers(&self, entry: ActivityId) -> bool {
        match self.reach {
            Reach::Open => true,
            Reach::UpTo(last) => entry <= last,
        }
    }
}

/// The list of objects taken off the hub because they were marked.
#[async_trait]
pub trait MarkedRemovals: Send + Sync {
    /// Remember this removal of `object`.
    async fn record(&self, object: ObjectId, removal: &Removal) -> Result<()>;
    /// The removal of `object`, if there was one.
    async fn find(&self, object: ObjectId) -> Result<Option<Removal>>;
    /// Everything on the list.
    async fn all(&self) -> Result<Vec<(ObjectId, Removal)>>;
}

/// [`MarkedRemovals`] over a `TursoStore`.
pub struct StoreMarked {
    marks: Arc<TursoStore>,
}

impl StoreMarked {
    /// Use `marks`, a store for this alone: it keeps no history.
    pub async fn new(marks: Arc<TursoStore>) -> Result<Self> {
        marks.set_retention(Retention::steps(0)).await?;
        Ok(Self { marks })
    }
}

#[async_trait]
impl MarkedRemovals for StoreMarked {
    async fn record(&self, object: ObjectId, removal: &Removal) -> Result<()> {
        let mut envelope = Envelope::new(MARKED_TYPE, object.to_string());
        envelope.id = object;
        let content = serde_json::to_value(removal)?;
        let entry = StoredObject::new(envelope, MARKED_CONTENT_TYPE, content);
        let written = self.marks.put(entry, MARKED_ACTOR).await?;
        self.marks.mark_synced(written.id).await?;
        self.marks.compact().await?;
        Ok(())
    }

    async fn find(&self, object: ObjectId) -> Result<Option<Removal>> {
        let found = self.marks.get(object).await?;
        let Some(entry) = found.filter(|o| o.envelope.object_type == MARKED_TYPE) else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_value(entry.content)?))
    }

    async fn all(&self) -> Result<Vec<(ObjectId, Removal)>> {
        self.marks
            .query(&Query::new().of_type(MARKED_TYPE))
            .await?
            .into_iter()
            .map(|o| Ok((o.envelope.id, serde_json::from_value(o.content)?)))
            .collect()
    }
}
