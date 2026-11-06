//! Collecting what programs push.
//!
//! Pattern: Command executor — `push` walks a program's activities in order and applies each one
//! to the hub's object store (`apply.rs`), idempotent by the program's `last_accepted` cursor. One
//! bad entry is a problem, and the cursor moves past it. A failure that may pass is not the entry's
//! fault: the push stops there and the cursor stays in front of it.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::{Problem, PushOutcome, Pushed, too_large};
use fs_store::traits::{ActivityLog, ObjectRepository};

use super::activity::ActivitySink;
use super::apply::Applied;
use super::marked::MarkedRemovals;
use super::originals::OriginalTimes;
use super::registry::ProgramRegistry;
use super::state::now_ms;
use crate::Result;

/// [`ActivitySink`] that stores pushed objects in the hub's store.
pub struct ObjectCollector {
    pub(super) registry: Arc<dyn ProgramRegistry>,
    pub(super) objects: Arc<dyn ObjectRepository>,
    pub(super) log: Arc<dyn ActivityLog>,
    pub(super) originals: Arc<dyn OriginalTimes>,
    pub(super) marked: Arc<dyn MarkedRemovals>,
}

impl ObjectCollector {
    /// Wire the collaborators.
    pub fn new(
        registry: Arc<dyn ProgramRegistry>,
        objects: Arc<dyn ObjectRepository>,
        log: Arc<dyn ActivityLog>,
        originals: Arc<dyn OriginalTimes>,
        marked: Arc<dyn MarkedRemovals>,
    ) -> Self {
        Self {
            registry,
            objects,
            log,
            originals,
            marked,
        }
    }
}

#[async_trait]
impl ActivitySink for ObjectCollector {
    async fn push(&self, program: &str, mut activities: Vec<Pushed>) -> Result<PushOutcome> {
        let mut st = self.registry.lock(program).await?;
        activities.sort_by_key(|p| p.activity.id);
        let mut out = PushOutcome::default();
        for Pushed {
            activity: mut a,
            original_at,
        } in activities
        {
            if st.last_accepted.is_some_and(|l| a.id <= l) {
                out.skipped += 1;
                continue;
            }
            match self.apply_one(&st.manifest, &mut a).await {
                Ok(Applied::Done(logged)) => {
                    out.accepted += 1;
                    if let Some(at) = original_at {
                        // The change is in; only when it happened is lost if this fails.
                        if let Err(e) = self.originals.record(logged, at).await {
                            tracing::warn!("original time of {logged} not kept: {e}");
                        }
                    }
                }
                Ok(Applied::DoneSensitive) => out.accepted += 1,
                Ok(Applied::Same) => out.skipped += 1,
                Ok(Applied::Withheld) => out.withheld += 1,
                Ok(Applied::TooLarge(bytes)) => out.problems.push(too_large(&a, bytes)),
                Err(e) if e.is_transient() => {
                    // What was applied stays applied; this entry comes again.
                    st.save().await?;
                    return Err(e);
                }
                Err(e) => {
                    // One bad entry must not block the log behind it: report and move on.
                    out.problems.push(Problem::new(
                        "about:blank",
                        "Push problem",
                        format!("{} {}: {e}", a.kind.as_str(), a.id),
                        422,
                    ));
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
