//! Handing out activities to programs.
//!
//! Pattern: Repository (read side) + Observer — scans the hub's activity log from the program's
//! cursor, filtered by [`Visibility`] and cut by count and size; `changes` exposes the store's change
//! feed for subscribers. An answer ends before the activity that would not fit, so `last` and `more`
//! mean what they always meant: nothing is lost or repeated across pages.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::{PageBudget, PullBatch, ScanSize, activity_weight};
use fs_objects::traits::{ActivityLog, ChangeFeed};
use fs_objects::{Activity, ActivityFilter, ActivityId, ObjectId};
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
    pub(super) log: Arc<dyn ActivityLog>,
    feed: Arc<dyn ChangeFeed>,
    pub(super) visibility: Visibility,
    pub(super) originals: Arc<dyn OriginalTimes>,
    pub(super) marked: Arc<dyn MarkedRemovals>,
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
                let Some(way) = self.way_out(program, &reads, &a).await? else {
                    out.last = Some(a.id);
                    continue;
                };
                if self.superseded(program, &a).await? {
                    out.last = Some(a.id);
                    continue;
                }
                if out.activities.len() >= limit {
                    out.more = true;
                    break 'scan;
                }
                let note = self.prepare(&mut a, way, envelope_only).await?;
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
        Ok(self.way_out(program, reads, a).await?.is_some())
    }

    async fn withdrawn(&self, object: ObjectId, entry: ActivityId) -> Result<bool> {
        let removal = self.marked.find(object).await?;
        Ok(removal.is_some_and(|r| r.covers(entry)))
    }

    fn changes(&self) -> broadcast::Receiver<Activity> {
        self.feed.subscribe()
    }
}
