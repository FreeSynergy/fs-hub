//! Who may see which activity.
//!
//! Pattern: Specification — a predicate over (program, declared reads, activity): other programs'
//! activities of a declared type, never one's own.

use std::sync::Arc;

use fs_store::Activity;
use fs_store::traits::ObjectRepository;

/// The visibility rule of `pull` and `subscribe`.
pub struct Visibility {
    objects: Arc<dyn ObjectRepository>,
}

impl Visibility {
    /// Rule over the hub's object store (needed for the type of log-only activities).
    pub fn new(objects: Arc<dyn ObjectRepository>) -> Self {
        Self { objects }
    }

    /// AS2 type an activity is about (state-changing: from the images; log-only: the stored object).
    pub async fn type_of(&self, a: &Activity) -> Option<String> {
        if a.kind.changes_state() {
            let cs = a.change_set().ok()?;
            cs.after.or(cs.before).map(|o| o.envelope.object_type)
        } else {
            self.objects
                .get(a.object)
                .await
                .ok()
                .flatten()
                .map(|o| o.envelope.object_type)
        }
    }

    /// Would `program` (with these `reads`) get this activity?
    pub async fn allows(&self, program: &str, reads: &[String], a: &Activity) -> bool {
        if a.actor == program {
            return false;
        }
        match self.type_of(a).await {
            Some(t) => reads.contains(&t),
            None => false,
        }
    }
}
