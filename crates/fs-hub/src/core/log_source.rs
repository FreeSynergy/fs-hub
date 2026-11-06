//! Handing out activities to programs.
//!
//! Pattern: Repository (read side) + Observer — scans the hub's activity log from the program's
//! cursor, filtered by [`Visibility`] and cut by count and size; `changes` exposes the store's change
//! feed for subscribers. An answer ends before the activity that would not fit, so `last` and `more`
//! mean what they always meant: nothing is lost or repeated across pages.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::{MAX_ACTIVITY_BYTES, PageBudget, PullBatch, PullNote, activity_weight};
use fs_store::traits::{ActivityLog, ChangeFeed};
use fs_store::{Activity, ActivityFilter, ActivityId, Envelope, Timestamp};
use serde_json::Value;
use tokio::sync::broadcast;

use super::activity::ActivitySource;
use super::originals::OriginalTimes;
use super::registry::ProgramRegistry;
use super::state::now_ms;
use super::visibility::Visibility;
use crate::Result;

/// Read from the log at once. Small, because every entry may hold two large images, and what does
/// not fit the page is read again for the next one.
const PULL_SCAN_BATCH: usize = 32;

/// [`ActivitySource`] over the hub's log.
pub struct LogSource {
    registry: Arc<dyn ProgramRegistry>,
    log: Arc<dyn ActivityLog>,
    feed: Arc<dyn ChangeFeed>,
    visibility: Visibility,
    originals: Arc<dyn OriginalTimes>,
}

impl LogSource {
    /// Wire the collaborators.
    pub fn new(
        registry: Arc<dyn ProgramRegistry>,
        log: Arc<dyn ActivityLog>,
        feed: Arc<dyn ChangeFeed>,
        visibility: Visibility,
        originals: Arc<dyn OriginalTimes>,
    ) -> Self {
        Self {
            registry,
            log,
            feed,
            visibility,
            originals,
        }
    }

    /// When the program did it. Never for a sensitive object, whatever is on file (ADR 0029).
    async fn original_at(&self, id: ActivityId, envelope: &Envelope) -> Option<Timestamp> {
        if !envelope.sensitive.is_empty() {
            return None;
        }
        match self.originals.find(id).await {
            Ok(at) => at,
            Err(e) => {
                tracing::warn!("original time of {id} not read: {e}");
                None
            }
        }
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
        'scan: loop {
            let batch = self
                .log
                .activities(&ActivityFilter {
                    after: cursor,
                    limit: Some(PULL_SCAN_BATCH),
                    ..Default::default()
                })
                .await?;
            let n = batch.len();
            for mut a in batch {
                cursor = Some(a.id);
                let Some(envelope) = self.visibility.visible_envelope(program, &reads, &a).await
                else {
                    out.last = Some(a.id);
                    continue;
                };
                if out.activities.len() >= limit {
                    out.more = true;
                    break 'scan;
                }
                if envelope_only {
                    a.payload = Value::Null;
                }
                let mut weight = activity_weight(&a, Some(&envelope));
                if weight > MAX_ACTIVITY_BYTES {
                    // Older than the limits: no message holds it. Its reader moves on and sees why.
                    tracing::warn!("{} {} held back: {weight} bytes", a.kind.as_str(), a.id);
                    a.payload = Value::Null;
                    weight = activity_weight(&a, Some(&envelope));
                }
                if !budget.admits(weight) {
                    out.more = true;
                    break 'scan;
                }
                let original_at = self.original_at(a.id, &envelope).await;
                out.last = Some(a.id);
                out.notes.insert(
                    a.id,
                    PullNote {
                        envelope,
                        original_at,
                    },
                );
                out.activities.push(a);
            }
            if n < PULL_SCAN_BATCH {
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

    async fn visible_to(&self, program: &str, reads: &[String], a: &Activity) -> bool {
        self.visibility.allows(program, reads, a).await
    }

    fn changes(&self) -> broadcast::Receiver<Activity> {
        self.feed.subscribe()
    }
}
