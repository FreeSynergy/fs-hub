//! How an entry of the hub's log leaves it.
//!
//! Everything a reader gets passes here: who may have it at all, what a mark leaves of it, and what is
//! blanked. A covered entry passes as invisible, except the delete, which goes plain.
//!
//! Pattern: Specification (`way_out`) + Template method (`prepare`).

use chrono::{DateTime, Utc};
use fs_hub_proto::{
    MAX_ACTIVITY_BYTES, PullNote, activity_weight, blank_for_delivery, blank_images,
    plain_delete_envelope,
};
use fs_objects::{Activity, ActivityFilter, ActivityKind, Envelope};
use serde_json::Value;

use super::log_source::LogSource;
use crate::Result;

/// How an entry leaves the hub.
pub(super) enum WayOut {
    /// As it is, blanked.
    Normal(Box<Envelope>),
    /// The delete of an object marked later: that it is gone, nothing else.
    PlainDelete,
}

impl LogSource {
    /// Does this entry go to `program`, and how? Everything leaving the hub passes here.
    pub(super) async fn way_out(
        &self,
        program: &str,
        reads: &[String],
        a: &Activity,
    ) -> Result<Option<WayOut>> {
        let Some(envelope) = self.visibility.visible_envelope(program, reads, a).await? else {
            return Ok(None);
        };
        let Some(removal) = self.marked.find(a.object).await?.filter(|r| r.covers(a.id)) else {
            return Ok(Some(WayOut::Normal(Box::new(envelope))));
        };
        // From before a mark only that it is gone goes out, and a mark never deletes the object of the
        // program that wrote it.
        let tells = a.kind == ActivityKind::Delete && removal.origin != program;
        Ok(tells.then_some(WayOut::PlainDelete))
    }

    /// The program itself changed this object later: its version wins at the hub, so this older one must
    /// not win at the program, or two concurrent edits differ for good. Its own log-only entries change
    /// nothing, and must not swallow what another program wrote.
    pub(super) async fn superseded(&self, program: &str, a: &Activity) -> Result<bool> {
        let mut after = a.id;
        loop {
            let own_later = self
                .log
                .activities(&ActivityFilter {
                    after: Some(after),
                    object: Some(a.object),
                    actor: Some(program.to_string()),
                    limit: Some(1),
                    ..Default::default()
                })
                .await?;
            let Some(entry) = own_later.first() else {
                return Ok(false);
            };
            if entry.kind.changes_state() {
                return Ok(true);
            }
            after = entry.id;
        }
    }

    /// Make an entry ready to go out.
    pub(super) async fn prepare(
        &self,
        a: &mut Activity,
        way: WayOut,
        envelope_only: bool,
    ) -> Result<PullNote> {
        let mut envelope = match way {
            WayOut::PlainDelete => {
                a.payload = Value::Null;
                a.actor = String::new();
                a.at = DateTime::<Utc>::UNIX_EPOCH;
                return Ok(PullNote {
                    envelope: plain_delete_envelope(a.object),
                    original_at: None,
                });
            }
            WayOut::Normal(envelope) => *envelope,
        };
        blank_for_delivery(&mut envelope);
        if envelope_only || !a.kind.changes_state() {
            a.payload = Value::Null; // the payload of a log-only entry is not delivered (provisional)
        } else {
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
