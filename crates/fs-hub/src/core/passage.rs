//! Who may pass a mark that holds.
//!
//! Pattern: Specification — the rule is one function of the removal on the list, the program and what
//! its change proves; nothing else decides.

use fs_objects::ObjectId;

use super::collector::ObjectCollector;
use super::marked::Removal;
use crate::Result;

/// What a program's change proves about a mark that holds.
pub(super) struct Proof {
    /// The `before` image is marked: the program knows the mark.
    pub(super) was_sensitive: bool,
    /// An update without any `before` image: the history is gone (`sync/redacted.rs` of the client).
    pub(super) says_unmarked: bool,
}

/// What a mark says to an unmarked put.
pub(super) enum Passage {
    /// Nothing in the way.
    Free,
    /// The mark was lifted by another program than the owner: its old version stays out.
    LiftedOnly,
    /// The owner's write: it waits.
    Held,
    /// Refused.
    Refused,
}

impl ObjectCollector {
    /// While a mark holds, who may lift it. With a change whose `before` image is sensitive (the program
    /// knows the mark exists): the owner and the program that set it. Without any `before` image (the
    /// history was purged, so the program can prove nothing): only the program that set it, and the owner
    /// if it set the mark itself; it cannot know of a mark another program set, and an unmarked copy from
    /// it must not undo that. In every other case the owner goes on writing; that waits, it is not refused,
    /// and nothing it holds is touched. A stale copy from anywhere else must not bring the secret back.
    /// When the mark lifts, the owner's object must win, so what the marking program brings along stays
    /// out: it may be older than what the owner holds, which the hub cannot tell. (Program names are not
    /// authenticated yet, ADR 0035.)
    pub(super) async fn passage(
        &self,
        program: &str,
        id: ObjectId,
        proof: Proof,
    ) -> Result<Passage> {
        let Some(removal) = self.marked.find(id).await?.filter(|r| r.holds) else {
            return Ok(Passage::Free);
        };
        let is_owner = removal.origin == program;
        let is_marker = removal.by == program;
        let may_lift = if proof.was_sensitive {
            is_owner || is_marker
        } else {
            proof.says_unmarked && is_marker
        };
        if may_lift {
            let lifted = Removal {
                holds: false,
                ..removal
            };
            self.marked.record(id, &lifted).await?;
            return Ok(if is_owner {
                Passage::Free
            } else {
                Passage::LiftedOnly
            });
        }
        Ok(if is_owner {
            Passage::Held
        } else {
            Passage::Refused
        })
    }
}
