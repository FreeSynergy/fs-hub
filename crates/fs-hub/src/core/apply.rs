//! Applying one pushed activity to the hub's store.
//!
//! Pattern: Command (executed) — each activity is a change (create/update/delete/log-only) with
//! before/after images; the rules (sensitive, declared `writes`, size, marks, who decides which field,
//! identical content) live here, the removal of marked objects in `apply_mark.rs`. The payload is taken,
//! not copied: it may hold two large images.

use fs_hub_proto::{
    MAX_OBJECT_BYTES, Manifest, json_len, keep_held_back, same_as_written, same_content,
};
use fs_store::{Activity, ActivityId, ChangeSet, ObjectId};

use super::collector::ObjectCollector;
use super::marked::Removal;
use super::proposals::origin_of;
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
    /// The object was marked by another program, or this is not the way back.
    Marked,
}

impl ObjectCollector {
    pub(super) async fn apply_one(&self, manifest: &Manifest, a: &mut Activity) -> Result<Applied> {
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
            let payload = std::mem::take(&mut a.payload);
            let logged = self.log.log(a.kind, a.object, program, payload).await?;
            return Ok(Applied::Done(logged.id));
        }
        let cs: ChangeSet = serde_json::from_value(std::mem::take(&mut a.payload))?;
        let was_sensitive = cs
            .before
            .as_ref()
            .is_some_and(|o| !o.envelope.sensitive.is_empty());
        let Some(mut obj) = cs.after else {
            return self.apply_delete(manifest, a.object, was_sensitive).await;
        };
        let id = obj.envelope.id;
        if !obj.envelope.sensitive.is_empty() {
            return self.apply_mark(manifest, id).await;
        }
        if !manifest.writes.contains(&obj.envelope.object_type) {
            return Ok(Applied::Withheld);
        }
        let bytes = json_len(&obj);
        if bytes > MAX_OBJECT_BYTES {
            return Ok(Applied::TooLarge(bytes));
        }
        if !self.may_come_back(program, id, was_sensitive).await? {
            return Ok(Applied::Marked);
        }
        if let Some(have) = self.objects.get(id).await? {
            // The program that first wrote it decides the fields a reader never gets. From any other
            // they are blank, and that must not overwrite what the hub holds.
            let same = if origin_of(self.log.as_ref(), id).await? == program {
                same_as_written(&have, &obj)
            } else {
                keep_held_back(&have, &mut obj);
                same_content(&have, &obj)
            };
            if same {
                return Ok(Applied::Same); // e.g. a program re-pushing what it pulled
            }
        }
        let logged = self.objects.put(obj, program).await?.id;
        Ok(if was_sensitive {
            Applied::DoneSensitive
        } else {
            Applied::Done(logged)
        })
    }

    /// While a mark holds, only the program that set it can bring the object back, with a change that
    /// says it was sensitive. A stale copy from anywhere else must not bring the secret back.
    async fn may_come_back(
        &self,
        program: &str,
        id: ObjectId,
        was_sensitive: bool,
    ) -> Result<bool> {
        let Some(removal) = self.marked.find(id).await? else {
            return Ok(true);
        };
        if !removal.holds {
            return Ok(true);
        }
        if removal.by != program || !was_sensitive {
            return Ok(false);
        }
        let lifted = Removal {
            holds: false,
            ..removal
        };
        self.marked.record(id, &lifted).await?;
        Ok(true)
    }

    async fn apply_delete(
        &self,
        manifest: &Manifest,
        object: ObjectId,
        was_sensitive: bool,
    ) -> Result<Applied> {
        let Some(have) = self.objects.get(object).await? else {
            if was_sensitive {
                self.mark_without_copy(manifest, object).await?;
            }
            return Ok(Applied::Same);
        };
        if !manifest.writes.contains(&have.envelope.object_type) {
            return Ok(Applied::Withheld);
        }
        if was_sensitive {
            self.remove_marked(&manifest.program, object).await?;
            return Ok(Applied::DoneSensitive);
        }
        let logged = self.objects.delete(object, &manifest.program).await?;
        Ok(Applied::Done(logged.id))
    }
}
