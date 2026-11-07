//! Applying one pushed activity to the hub's store.
//!
//! Pattern: Command (executed) — each activity is a change (create/update/delete/log-only) with
//! before/after images; the rules (sensitive, declared `writes`, size, marks, who decides which field,
//! identical content) live here; a redacted entry (purged history) is skipped, never read as a delete, the removal of marked objects in `apply_mark.rs`. The payload is taken,
//! not copied: it may hold two large images.

use fs_hub_proto::{
    MAX_OBJECT_BYTES, Manifest, is_redacted, json_len, keep_held_back, same_as_written,
    same_content,
};
use fs_objects::{Activity, ActivityId, ActivityKind, ChangeSet, ObjectId};

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
    /// The object was marked by another program, and this program may not lift that.
    Marked,
    /// This program first wrote the object, and another program's mark keeps it away for now.
    Held,
}

/// What a mark says to an unmarked put.
enum Passage {
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
    pub(super) async fn apply_one(&self, manifest: &Manifest, a: &mut Activity) -> Result<Applied> {
        let program = manifest.program.as_str();
        if is_redacted(a) {
            // Purged history carries no state and is not a deletion: the hub's copy stays as it is. The
            // client sends the object's current copy in its place (`fs-hub-client` `sync/push.rs`).
            return Ok(Applied::Same);
        }
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
        // A change that has no `before` image at all is the current copy of an object whose history was
        // purged (`fs-hub-client` `sync/redacted.rs`): the owner's state is then all there is to go by.
        let says_unmarked = a.kind == ActivityKind::Update && cs.before.is_none();
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
        let held = self.objects.get(id).await?;
        // The type it has now counts too, or a program could overwrite what it may not write by naming
        // another type. Without a copy, the log still knows.
        let current_type = match &held {
            Some(have) => Some(have.envelope.object_type.clone()),
            None => self.history_of(id).await?.object_type,
        };
        if current_type.is_some_and(|t| !manifest.writes.contains(&t)) {
            return Ok(Applied::Withheld);
        }
        let bytes = json_len(&obj);
        if bytes > MAX_OBJECT_BYTES {
            return Ok(Applied::TooLarge(bytes));
        }
        match self
            .passage(program, id, was_sensitive || says_unmarked)
            .await?
        {
            Passage::Free => {}
            Passage::LiftedOnly => return Ok(Applied::Same),
            Passage::Held => return Ok(Applied::Held),
            Passage::Refused => return Ok(Applied::Marked),
        }
        if let Some(have) = held {
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

    /// While a mark holds: the owner and the program that set it can lift it, with a change that says it
    /// was sensitive (or whose history is gone, then `after` unmarked says it), and nobody else. The owner goes on writing; that waits, it is not refused. A stale
    /// copy from anywhere else must not bring the secret back. And when the mark lifts, the owner's object
    /// must win, so what the marking program brings along stays out: it may be older than what the owner
    /// holds, which the hub cannot tell.
    async fn passage(&self, program: &str, id: ObjectId, lifts: bool) -> Result<Passage> {
        let Some(removal) = self.marked.find(id).await?.filter(|r| r.holds) else {
            return Ok(Passage::Free);
        };
        let is_owner = removal.origin == program;
        if lifts && (is_owner || removal.by == program) {
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
