//! Handing out activities to programs.
//!
//! Pattern: Repository (read side) + Observer — scans the hub's activity log from the program's
//! cursor, filtered by [`Visibility`] and cut by count and size; `changes` exposes the store's change
//! feed for subscribers. An answer ends before the activity that would not fit, so `last` and `more`
//! mean what they always meant: nothing is lost or repeated across pages.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::{
    MAX_ACTIVITY_BYTES, PageBudget, PullBatch, PullNote, ScanSize, activity_weight,
    blank_for_delivery, blank_images, sensitive_stub_envelope,
};
use fs_store::traits::{ActivityLog, ChangeFeed};
use fs_store::{Activity, ActivityFilter, ActivityId, Envelope};
use serde_json::Value;
use tokio::sync::broadcast;

use super::activity::ActivitySource;
use super::marked::MarkedRemovals;
use super::originals::OriginalTimes;
use super::registry::ProgramRegistry;
use super::state::now_ms;
use super::visibility::Visibility;
use crate::Result;

/// [`ActivitySource`] over the hub's log.
pub struct LogSource {
    registry: Arc<dyn ProgramRegistry>,
    log: Arc<dyn ActivityLog>,
    feed: Arc<dyn ChangeFeed>,
    visibility: Visibility,
    originals: Arc<dyn OriginalTimes>,
    marked: Arc<dyn MarkedRemovals>,
}

impl LogSource {
    /// Wire the collaborators.
    pub fn new(
        registry: Arc<dyn ProgramRegistry>,
        log: Arc<dyn ActivityLog>,
        feed: Arc<dyn ChangeFeed>,
        visibility: Visibility,
        originals: Arc<dyn OriginalTimes>,
        marked: Arc<dyn MarkedRemovals>,
    ) -> Self {
        Self {
            registry,
            log,
            feed,
            visibility,
            originals,
            marked,
        }
    }

    /// Make an entry ready to go out: everything leaving the hub passes here.
    async fn prepare(
        &self,
        a: &mut Activity,
        mut envelope: Envelope,
        envelope_only: bool,
    ) -> Result<PullNote> {
        if self.marked.covers(a.object, a.id).await? {
            // From before the mark: that it happened, nothing of what it held.
            a.payload = Value::Null;
            return Ok(PullNote {
                envelope: sensitive_stub_envelope(a.object),
                original_at: None,
            });
        }
        blank_for_delivery(&mut envelope);
        if envelope_only {
            a.payload = Value::Null;
        } else if a.kind.changes_state() {
            blank_images(&mut a.payload);
        }
        if activity_weight(a, Some(&envelope)) > MAX_ACTIVITY_BYTES {
            // Older than the limits. The reader only needs `after`, if that fits.
            tracing::warn!("{} {} is too large for a message", a.kind.as_str(), a.id);
            if let Some(before) = a.payload.get_mut("before") {
                *before = Value::Null;
            }
            if activity_weight(a, Some(&envelope)) > MAX_ACTIVITY_BYTES {
                a.payload = Value::Null;
            }
        }
        let original_at = if envelope.sensitive.is_empty() {
            self.originals.find(a.id).await?
        } else {
            None // whatever is on file (ADR 0029)
        };
        Ok(PullNote {
            envelope,
            original_at,
        })
    }
}

#[async_trait]
impl ActivitySource for LogSource {
    async fn pull(
        &self,
        program: &str,
        since: Option<ActivityId>,
        limit: usize,
        envelope_only: bool,
    ) -> Result<PullBatch> {
        let (reads, mut cursor) = {
            let mut st = self.registry.lock(program).await?;
            st.last_seen_ms = now_ms();
            (st.manifest.reads.clone(), since.or(st.pull_cursor))
        };
        let limit = limit.clamp(1, 1000);
        let mut out = PullBatch {
            last: cursor,
            ..PullBatch::default()
        };
        let mut budget = PageBudget::default();
        let mut scan = ScanSize::default();
        'scan: loop {
            let asked = scan.next();
            let batch = self
                .log
                .activities(&ActivityFilter {
                    after: cursor,
                    limit: Some(asked),
                    ..Default::default()
                })
                .await?;
            let n = batch.len();
            for mut a in batch {
                cursor = Some(a.id);
                scan.saw(&a.payload);
                let visible = self
                    .visibility
                    .visible_envelope(program, &reads, &a)
                    .await?;
                let Some(envelope) = visible else {
                    out.last = Some(a.id);
                    continue;
                };
                if out.activities.len() >= limit {
                    out.more = true;
                    break 'scan;
                }
                let note = self.prepare(&mut a, envelope, envelope_only).await?;
                if !budget.admits(activity_weight(&a, Some(&note.envelope))) {
                    out.more = true;
                    break 'scan;
                }
                out.last = Some(a.id);
                out.notes.insert(a.id, note);
                out.activities.push(a);
            }
            if n < asked {
                break;
            }
        }
        Ok(out)
    }

    async fn ack(&self, program: &str, up_to: ActivityId) -> Result<()> {
        let mut st = self.registry.lock(program).await?;
        if st.pull_cursor.is_none_or(|c| up_to > c) {
            st.pull_cursor = Some(up_to);
            st.save().await?;
        }
        Ok(())
    }

    async fn visible_to(&self, program: &str, reads: &[String], a: &Activity) -> Result<bool> {
        self.visibility.allows(program, reads, a).await
    }

    fn changes(&self) -> broadcast::Receiver<Activity> {
        self.feed.subscribe()
    }
}
