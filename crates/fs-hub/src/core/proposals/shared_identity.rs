//! The first proposal strategy: shared e-mail or phone across programs.
//!
//! Pattern: Strategy implementation — `Profile` objects of different programs sharing a normalised
//! e-mail or phone number (>= 6 digits), same `kind`, not groups, not already linked.

use async_trait::async_trait;
use fs_hub_proto::Proposal;
use fs_objects::traits::{ActivityLog, ObjectQuery};

use super::ProposalEngine;
use super::candidates::by_identity_key;
use super::linked::linked_pairs;
use super::pairing::pair_up;
use crate::Result;

/// E-mail / phone matching (the programs already normalise their index fields).
#[derive(Clone, Copy, Debug, Default)]
pub struct SharedIdentity;

#[async_trait]
impl ProposalEngine for SharedIdentity {
    async fn propose(
        &self,
        objects: &dyn ObjectQuery,
        log: &dyn ActivityLog,
    ) -> Result<Vec<Proposal>> {
        let linked = linked_pairs(objects).await?;
        let by_value = by_identity_key(objects, log).await?;
        Ok(pair_up(by_value, &linked))
    }
}
