//! Who may see which activity.
//!
//! Pattern: Specification — a predicate over (program, declared reads, activity): other programs'
//! activities of a declared type, never one's own, never a redacted one (its type is unknown), never a never-read type
//! (`Relationship`), whatever an old manifest declared.

use std::sync::Arc;

use fs_hub_proto::{is_redacted, may_read, summary_of};
use fs_store::traits::ObjectRepository;
use fs_store::{Activity, Envelope};

use crate::Result;

/// The visibility rule of `pull` and `subscribe`.
pub struct Visibility {
    objects: Arc<dyn ObjectRepository>,
}

impl Visibility {
    /// Rule over the hub's object store (needed for the type of log-only activities).
    pub fn new(objects: Arc<dyn ObjectRepository>) -> Self {
        Self { objects }
    }

    /// Summary of the object an activity is about (state-changing: from the images; log-only: the
    /// stored object).
    pub async fn envelope_of(&self, a: &Activity) -> Result<Option<Envelope>> {
        if is_redacted(a) {
            return Ok(None); // the type is unknown, so nobody is entitled to it
        }
        if a.kind.changes_state() {
            return Ok(summary_of(a));
        }
        Ok(self.objects.get(a.object).await?.map(|o| o.envelope))
    }

    /// The summary, if `program` (with these `reads`) gets this activity. A store error is not "no":
    /// a cursor would move past the entry.
    pub async fn visible_envelope(
        &self,
        program: &str,
        reads: &[String],
        a: &Activity,
    ) -> Result<Option<Envelope>> {
        if a.actor == program {
            return Ok(None);
        }
        let envelope = self.envelope_of(a).await?;
        Ok(envelope.filter(|e| reads.contains(&e.object_type) && may_read(&e.object_type)))
    }

    /// Would `program` (with these `reads`) get this activity?
    pub async fn allows(&self, program: &str, reads: &[String], a: &Activity) -> Result<bool> {
        Ok(self.visible_envelope(program, reads, a).await?.is_some())
    }
}
