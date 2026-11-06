//! Handing out activities to programs.
//!
//! Pattern: Repository (read side) + Observer — scans the hub's activity log from the program's
//! cursor, filtered by [`Visibility`]; `changes` exposes the store's change feed for subscribers.

use std::sync::Arc;

use async_trait::async_trait;
use fs_hub_proto::PullBatch;
use fs_store::traits::{ActivityLog, ChangeFeed};
use fs_store::{Activity, ActivityFilter, ActivityId};
use tokio::sync::broadcast;

use super::activity::ActivitySource;
use super::registry::ProgramRegistry;
use super::state::now_ms;
use super::visibility::Visibility;
use crate::Result;

const PULL_SCAN_BATCH: usize = 200;

/// [`ActivitySource`] over the hub's log.
pub struct LogSource {
    registry: Arc<dyn ProgramRegistry>,
    log: Arc<dyn ActivityLog>,
    feed: Arc<dyn ChangeFeed>,
    visibility: Visibility,
}

impl LogSource {
    /// Wire the collaborators.
    pub fn new(
        registry: Arc<dyn ProgramRegistry>,
        log: Arc<dyn ActivityLog>,
        feed: Arc<dyn ChangeFeed>,
        visibility: Visibility,
    ) -> Self {
        Self {
            registry,
            log,
            feed,
            visibility,
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
            for a in batch {
                cursor = Some(a.id);
                if self.visibility.allows(program, &reads, &a).await {
                    if out.activities.len() >= limit {
                        out.more = true;
                        break 'scan;
                    }
                    out.last = Some(a.id);
                    out.activities.push(a);
                } else {
                    out.last = Some(a.id);
                }
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
