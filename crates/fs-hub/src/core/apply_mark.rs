//! Taking a marked object off the hub.
//!
//! A sensitive object is withheld at any size, and any older copy goes, also when the hub holds no copy
//! any more: its log still has the old content. The order is what makes this safe: the list comes first,
//! open, so from then on nothing of the object goes out, whatever happens next; then the delete; then the
//! times; and only then the list closes. After a crash the same push comes again, finds no copy and
//! finishes the work. A program may only do this to objects of a type it declared in `writes`.
//!
//! Pattern: Command (executed), with a recoverable order of steps.

use fs_hub_proto::{Manifest, summary_of};
use fs_objects::{ActivityFilter, ActivityId, ObjectId};

use super::apply::Applied;
use super::collector::ObjectCollector;
use super::marked::{Reach, Removal};
use super::proposals::origin_of;
use crate::Result;

/// What the log knows of an object.
#[derive(Default)]
pub(super) struct History {
    pub entries: Vec<ActivityId>,
    pub object_type: Option<String>,
}

impl ObjectCollector {
    pub(super) async fn apply_mark(&self, manifest: &Manifest, id: ObjectId) -> Result<Applied> {
        match self.objects.get(id).await? {
            Some(have) => {
                if manifest.writes.contains(&have.envelope.object_type) {
                    self.remove_marked(&manifest.program, id).await?;
                }
            }
            None => self.mark_without_copy(manifest, id).await?,
        }
        Ok(Applied::Withheld)
    }

    /// The hub holds a copy: list, delete, forget, close. The origin goes on the list now: the log it
    /// comes from will be purged.
    pub(super) async fn remove_marked(&self, program: &str, id: ObjectId) -> Result<()> {
        let removal = Removal {
            reach: Reach::Open,
            by: program.to_string(),
            origin: origin_of(self.log.as_ref(), id).await?,
            holds: true,
        };
        self.take_off(id, removal).await
    }

    async fn take_off(&self, id: ObjectId, mut removal: Removal) -> Result<()> {
        removal.reach = Reach::Open;
        self.marked.record(id, &removal).await?;
        let logged = self.objects.delete(id, &removal.by).await?;
        self.forget_times(id).await?;
        removal.reach = Reach::UpTo(logged.id);
        self.marked.record(id, &removal).await
    }

    /// At start: a mark that holds with a copy here. `marks.db` can be newer than `objects.db`, after a
    /// restored backup, or a crash left the removal open.
    pub async fn finish_removals(&self) -> Result<()> {
        for (id, removal) in self.marked.all().await? {
            if !removal.holds {
                continue;
            }
            if self.objects.get(id).await?.is_some() {
                self.take_off(id, removal).await?;
            } else if removal.reach == Reach::Open {
                self.forget_times(id).await?;
                let last = self.history_of(id).await?.entries.pop();
                if let Some(last) = last {
                    let closed = Removal {
                        reach: Reach::UpTo(last),
                        ..removal
                    };
                    self.marked.record(id, &closed).await?;
                }
            }
        }
        Ok(())
    }

    /// The hub holds no copy, but its log may have old content; or a crash left the removal open.
    pub(super) async fn mark_without_copy(&self, manifest: &Manifest, id: ObjectId) -> Result<()> {
        let listed = self.marked.find(id).await?;
        if listed
            .as_ref()
            .is_some_and(|r| r.holds && r.reach != Reach::Open)
        {
            return Ok(()); // nothing can have been logged since: while it holds, nothing is taken
        }
        let history = self.history_of(id).await?;
        let Some(&last) = history.entries.last() else {
            return Ok(()); // never here: nothing to keep from going out
        };
        let declared = history
            .object_type
            .is_some_and(|t| manifest.writes.contains(&t));
        if !declared {
            return Ok(());
        }
        let (by, origin) = match listed {
            Some(r) if r.holds => (r.by, r.origin),
            _ => (
                manifest.program.clone(),
                origin_of(self.log.as_ref(), id).await?,
            ),
        };
        for entry in &history.entries {
            self.originals.forget(*entry).await?;
        }
        let removal = Removal {
            reach: Reach::UpTo(last),
            by,
            origin,
            holds: true,
        };
        self.marked.record(id, &removal).await
    }

    async fn forget_times(&self, id: ObjectId) -> Result<()> {
        for entry in self.history_of(id).await?.entries {
            self.originals.forget(entry).await?;
        }
        Ok(())
    }

    /// The log entries about `object`, reading one at a time: each may be large.
    pub(super) async fn history_of(&self, object: ObjectId) -> Result<History> {
        let mut history = History::default();
        loop {
            let filter = ActivityFilter {
                after: history.entries.last().copied(),
                object: Some(object),
                limit: Some(1),
                ..Default::default()
            };
            let Some(entry) = self.log.activities(&filter).await?.pop() else {
                return Ok(history);
            };
            history.entries.push(entry.id);
            if let Some(summary) = summary_of(&entry) {
                history.object_type = Some(summary.object_type);
            }
        }
    }
}
