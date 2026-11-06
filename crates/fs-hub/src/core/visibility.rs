//! Who may see which activity.
//!
//! Pattern: Specification — a predicate over (program, declared reads, activity): other programs'
//! activities of a declared type, never one's own.

use std::sync::Arc;

use fs_hub_proto::summary_of;
use fs_store::traits::ObjectRepository;
use fs_store::{Activity, Envelope};

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
    pub async fn envelope_of(&self, a: &Activity) -> Option<Envelope> {
        if a.kind.changes_state() {
            summary_of(a)
        } else {
            self.objects
                .get(a.object)
                .await
                .ok()
                .flatten()
                .map(|o| o.envelope)
        }
    }

    /// The summary, if `program` (with these `reads`) gets this activity.
    pub async fn visible_envelope(
        &self,
        program: &str,
        reads: &[String],
        a: &Activity,
    ) -> Option<Envelope> {
        if a.actor == program {
            return None;
        }
        self.envelope_of(a)
            .await
            .filter(|e| reads.contains(&e.object_type))
    }

    /// Would `program` (with these `reads`) get this activity?
    pub async fn allows(&self, program: &str, reads: &[String], a: &Activity) -> bool {
        self.visible_envelope(program, reads, a).await.is_some()
    }
}
