//! The two directions of activity flow.
//!
//! Pattern: Strategy (interface segregation) — `ActivitySink` takes what programs push,
//! `ActivitySource` hands out what others wrote; the RPC layer and tests depend on these traits,
//! the collector and log source are the implementations.

use async_trait::async_trait;
use fs_hub_proto::{PullBatch, PushOutcome};
use fs_store::{Activity, ActivityId};
use tokio::sync::broadcast;

use crate::Result;

/// Program -> hub.
#[async_trait]
pub trait ActivitySink: Send + Sync {
    /// Apply a program's activities. Idempotent: entries at or below the program's cursor are
    /// skipped. Sensitive objects (ADR 0029) are never collected; they are counted as `withheld`.
    async fn push(&self, program: &str, activities: Vec<Activity>) -> Result<PushOutcome>;
}

/// Hub -> program.
#[async_trait]
pub trait ActivitySource: Send + Sync {
    /// Activities of other programs after `since` (default: the hub-kept cursor), only declared
    /// `reads`. Does not move the cursor; the program calls [`ActivitySource::ack`].
    async fn pull(
        &self,
        program: &str,
        since: Option<ActivityId>,
        limit: usize,
    ) -> Result<PullBatch>;
    /// The program applied everything up to `up_to`; remember it (cursors only move forward).
    async fn ack(&self, program: &str, up_to: ActivityId) -> Result<()>;
    /// Would `program` (with these `reads`) get this activity?
    async fn visible_to(&self, program: &str, reads: &[String], a: &Activity) -> bool;
    /// Live feed of every activity the hub logs (for subscriptions; filter with `visible_to`).
    fn changes(&self) -> broadcast::Receiver<Activity>;
}
