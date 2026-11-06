//! Applying one pushed activity to the hub's store.
//!
//! Pattern: Command (executed) — each activity is a change (create/update/delete/log-only) with
//! before/after images; the rules (sensitive, declared `writes`, size, identical content) live here.
//! Sensitive comes first: such an object is withheld at any size, and an older copy still goes. The
//! payload is taken, not copied: it may hold two large images.

use fs_hub_proto::{MAX_OBJECT_BYTES, Manifest, json_len, keep_held_back, same_content};
use fs_store::{Activity, ActivityFilter, ActivityId, ChangeSet, ObjectId};

use super::collector::ObjectCollector;
use super::marked::Reach;
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
            if self.objects.get(a.object).await?.is_none() {
                self.finish_removal(a.object).await?;
                return Ok(Applied::Same);
            }
            if was_sensitive {
                self.remove_marked(a.object, program).await?;
                return Ok(Applied::DoneSensitive);
            }
            let logged = self.objects.delete(a.object, program).await?;
            return Ok(Applied::Done(logged.id));
        };
        if !obj.envelope.sensitive.is_empty() {
            // Never keep a copy; and drop an older, then-unmarked copy.
            if self.objects.get(obj.envelope.id).await?.is_some() {
                self.remove_marked(obj.envelope.id, program).await?;
            } else {
                self.finish_removal(obj.envelope.id).await?;
            }
            return Ok(Applied::Withheld);
        }
        if !manifest.writes.contains(&obj.envelope.object_type) {
            return Ok(Applied::Withheld);
        }
        if let Some(have) = self.objects.get(obj.envelope.id).await? {
            if same_content(&have, &obj) {
                return Ok(Applied::Same); // e.g. a program re-pushing what it pulled
            }
            // From a reader these fields are blank: that must not overwrite what the hub holds.
            keep_held_back(&have.envelope, &mut obj.envelope);
        }
        let bytes = json_len(&obj);
        if bytes > MAX_OBJECT_BYTES {
            return Ok(Applied::TooLarge(bytes));
        }
        let logged = self.objects.put(obj, program).await?.id;
        Ok(if was_sensitive {
            Applied::DoneSensitive
        } else {
            Applied::Done(logged)
        })
    }

    /// Remove the older copy of a marked object. The list comes first: from then on nothing of it goes
    /// out, whatever happens next.
    async fn remove_marked(&self, object: ObjectId, program: &str) -> Result<()> {
        self.marked.record(object, Reach::Open).await?;
        let logged = self.objects.delete(object, program).await?;
        self.marked.record(object, Reach::UpTo(logged.id)).await?;
        self.forget_times(object).await
    }

    /// A crash after the delete left the removal open: this push came again, and the copy is gone.
    async fn finish_removal(&self, object: ObjectId) -> Result<()> {
        if self.marked.find(object).await? == Some(Reach::Open) {
            let last = self.entries_about(object).await?.pop();
            if let Some(last) = last {
                self.marked.record(object, Reach::UpTo(last)).await?;
            }
            self.forget_times(object).await?;
        }
        Ok(())
    }

    async fn forget_times(&self, object: ObjectId) -> Result<()> {
        for entry in self.entries_about(object).await? {
            self.originals.forget(entry).await?;
        }
        Ok(())
    }

    /// The ids of the log entries about `object`, reading one at a time: each may be large.
    async fn entries_about(&self, object: ObjectId) -> Result<Vec<ActivityId>> {
        let mut ids = Vec::new();
        loop {
            let filter = ActivityFilter {
                after: ids.last().copied(),
                object: Some(object),
                limit: Some(1),
                ..Default::default()
            };
            match self.log.activities(&filter).await?.first() {
                Some(entry) => ids.push(entry.id),
                None => return Ok(ids),
            }
        }
    }
}
