//! Collecting what programs push.
//!
//! Pattern: Command executor — `push` walks a program's activities in order and applies each one
//! to the hub's object store (`apply.rs`), idempotent by the program's `last_accepted` cursor.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::PushOutcome;
use fs_store::Activity;
use fs_store::traits::{ActivityLog, ObjectRepository};

use super::activity::ActivitySink;
use super::apply::Applied;
use super::registry::ProgramRegistry;
use super::state::now_ms;
use crate::Result;

/// [`ActivitySink`] that stores pushed objects in the hub's store.
pub struct ObjectCollector {
    pub(super) registry: Arc<dyn ProgramRegistry>,
    pub(super) objects: Arc<dyn ObjectRepository>,
    pub(super) log: Arc<dyn ActivityLog>,
}

impl ObjectCollector {
    /// Wire the collaborators.
    pub fn new(
        registry: Arc<dyn ProgramRegistry>,
        objects: Arc<dyn ObjectRepository>,
        log: Arc<dyn ActivityLog>,
    ) -> Self {
        Self {
            registry,
            objects,
            log,
        }
    }
}

#[async_trait]
impl ActivitySink for ObjectCollector {
    async fn push(&self, program: &str, mut activities: Vec<Activity>) -> Result<PushOutcome> {
        let mut st = self.registry.lock(program).await?;
        activities.sort_by_key(|a| a.id);
        let mut out = PushOutcome::default();
        for a in activities {
            if st.last_accepted.is_some_and(|l| a.id <= l) {
                out.skipped += 1;
                continue;
            }
            match self.apply_one(&st.manifest, &a).await {
                Ok(Applied::Done) => out.accepted += 1,
                Ok(Applied::Same) => out.skipped += 1,
                Ok(Applied::Withheld) => out.withheld += 1,
                Err(e) => {
                    // One bad entry must not block the log behind it: report and move on.
                    out.problems
                        .push(format!("{} {}: {e}", a.kind.as_str(), a.id));
                }
            }
            st.last_accepted = Some(a.id);
        }
        st.last_seen_ms = now_ms();
        out.last_accepted = st.last_accepted;
        st.save().await?;
        Ok(out)
    }
}
