//! Applying one pushed activity to the hub's store.
//!
//! Pattern: Command (executed) — each activity is a change (create/update/delete/log-only) with
//! before/after images; the rules (sensitive, declared `writes`, identical content) live here.

use fs_hub_proto::{Manifest, same_content};
use fs_store::Activity;

use super::collector::ObjectCollector;
use crate::Result;

/// What applying an activity did.
pub(super) enum Applied {
    Done,
    Same,
    Withheld,
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
            self.log
                .log(a.kind, a.object, program, a.payload.clone())
                .await?;
            return Ok(Applied::Done);
        }
        let cs = a.change_set()?;
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
                if let Some(have) = self.objects.get(obj.envelope.id).await? {
                    if same_content(&have, &obj) {
                        return Ok(Applied::Same); // e.g. a program re-pushing what it pulled
                    }
                }
                self.objects.put(obj, program).await?;
                Ok(Applied::Done)
            }
            None => {
                if self.objects.get(a.object).await?.is_some() {
                    self.objects.delete(a.object, program).await?;
                    Ok(Applied::Done)
                } else {
                    Ok(Applied::Same)
                }
            }
        }
    }
}
