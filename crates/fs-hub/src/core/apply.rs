//! Applying one pushed activity to the hub's store.
//!
//! Pattern: Command (executed) — each activity is a change (create/update/delete/log-only) with
//! before/after images; the rules (sensitive, declared `writes`, size, identical content) live here.
//! Sensitive comes first: such an object is withheld at any size, and an older copy still goes.

use fs_hub_proto::{MAX_OBJECT_BYTES, Manifest, json_len, same_content};
use fs_store::{Activity, ActivityId};

use super::collector::ObjectCollector;
use crate::Result;

/// What applying an activity did.
pub(super) enum Applied {
    /// Logged by the hub under this id.
    Done(ActivityId),
    /// Logged, about an object that was sensitive in the program: it does not say when (ADR 0029).
    DoneSensitive,
    Same,
    Withheld,
    /// Above the limit, with this many bytes.
    TooLarge(usize),
}

impl ObjectCollector {
    pub(super) async fn apply_one(&self, manifest: &Manifest, a: &Activity) -> Result<Applied> {
        let program = manifest.program.as_str();
        if !a.kind.changes_state() {
            // Log-only (Invite, Accept, ...): keep it if we hold the object, else it has no anchor here.
            let Some(obj) = self.objects.get(a.object).await? else {
                return Ok(Applied::Same);
            };
            if !manifest.writes.contains(&obj.envelope.object_type) {
                return Ok(Applied::Withheld);
            }
            let bytes = json_len(&a.payload);
            if bytes > MAX_OBJECT_BYTES {
                return Ok(Applied::TooLarge(bytes));
            }
            let logged = self
                .log
                .log(a.kind, a.object, program, a.payload.clone())
                .await?;
            return Ok(Applied::Done(logged.id));
        }
        let cs = a.change_set()?;
        let was_sensitive = cs
            .before
            .as_ref()
            .is_some_and(|o| !o.envelope.sensitive.is_empty());
        let done = |id| {
            if was_sensitive {
                Applied::DoneSensitive
            } else {
                Applied::Done(id)
            }
        };
        match cs.after {
            Some(obj) => {
                if !obj.envelope.sensitive.is_empty() {
                    // Never keep a copy; and drop an older, then-unmarked copy.
                    if self.objects.get(obj.envelope.id).await?.is_some() {
                        self.objects.delete(obj.envelope.id, program).await?;
                    }
                    return Ok(Applied::Withheld);
                }
                if !manifest.writes.contains(&obj.envelope.object_type) {
                    return Ok(Applied::Withheld);
                }
                if let Some(have) = self.objects.get(obj.envelope.id).await?
                    && same_content(&have, &obj)
                {
                    return Ok(Applied::Same); // e.g. a program re-pushing what it pulled
                }
                let bytes = json_len(&obj);
                if bytes > MAX_OBJECT_BYTES {
                    return Ok(Applied::TooLarge(bytes));
                }
                Ok(done(self.objects.put(obj, program).await?.id))
            }
            None => {
                if self.objects.get(a.object).await?.is_some() {
                    Ok(done(self.objects.delete(a.object, program).await?.id))
                } else {
                    Ok(Applied::Same)
                }
            }
        }
    }
}
