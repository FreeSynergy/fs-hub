//! Same-person proposals.
//!
//! Pattern: Strategy — `ProposalEngine` is the interface; `SharedIdentity` (shared e-mail or
//! phone across programs) is the first implementation, others (name similarity, ...) can be
//! swapped in without touching the hub core.

mod candidates;
mod linked;
mod pairing;
mod shared_identity;

use async_trait::async_trait;
use fs_hub_proto::Proposal;
use fs_store::traits::{ActivityLog, ObjectQuery};

pub use shared_identity::SharedIdentity;

use crate::Result;

/// Finds "same person" candidates in the hub's collected objects. The hub only proposes; a human
/// confirms in a program, which writes the `Relationship` the engine then respects.
#[async_trait]
pub trait ProposalEngine: Send + Sync {
    /// Candidate pairs across programs, never involving sensitive objects (ADR 0029).
    async fn propose(
        &self,
        objects: &dyn ObjectQuery,
        log: &dyn ActivityLog,
    ) -> Result<Vec<Proposal>>;
}
