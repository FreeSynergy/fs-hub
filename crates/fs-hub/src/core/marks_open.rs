//! Opening the list of marked objects, which must not go missing unnoticed.
//!
//! `state.db` holds a marker from the moment the list exists. The marker without the file means the file
//! was lost, and the hub refuses to start: what the list keeps back could go out. Without a marker, this
//! is the first time, perhaps over the log of an older hub, which kept no list: there a marked object
//! looks like any deleted one, so every object whose last entry is a delete is listed, conservatively.
//! An ordinary delete of that time looks the same and loses its old entries for later readers. The marker
//! is written last, so a crash on the way only means doing this again.
//!
//! Pattern: Factory, with a one-time migration.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use fs_hub_proto::ScanSize;
use fs_objects::traits::{ActivityLog, ObjectQuery, ObjectRepository};
use fs_objects::{
    ActivityFilter, ActivityId, ActivityKind, Envelope, ObjectId, Query, StoredObject, TursoStore,
};

use super::marked::{MarkedRemovals, Reach, Removal, StoreMarked};
use crate::{Error, Result};

const MARKS_FILE: &str = "marks.db";
const MARKER_TYPE: &str = "MarksListExists";

/// Open `marks.db` in `dir`, as above.
pub(super) async fn open_marks(
    dir: &Path,
    state: &Arc<TursoStore>,
    log: &dyn ActivityLog,
) -> Result<Arc<StoreMarked>> {
    let path = dir.join(MARKS_FILE);
    let has_marker = state.count(&Query::new().of_type(MARKER_TYPE)).await? > 0;
    if has_marker && !path.exists() {
        return Err(Error::MarksLost(path.display().to_string()));
    }
    let marks = Arc::new(StoreMarked::new(Arc::new(TursoStore::open(&path).await?)).await?);
    if !has_marker {
        list_old_deletes(marks.as_ref(), log).await?;
        let marker = StoredObject::new(
            Envelope::new(MARKER_TYPE, MARKS_FILE),
            "application/json",
            serde_json::Value::Null,
        );
        state.put(marker, "hub").await?;
    }
    Ok(marks)
}

struct Seen {
    origin: String,
    last: ActivityId,
    last_kind: ActivityKind,
}

async fn list_old_deletes(marks: &dyn MarkedRemovals, log: &dyn ActivityLog) -> Result<()> {
    let mut objects: BTreeMap<ObjectId, Seen> = BTreeMap::new();
    let (mut after, mut scan) = (None, ScanSize::default());
    loop {
        let asked = scan.next();
        let batch = log
            .activities(&ActivityFilter {
                after,
                limit: Some(asked),
                ..Default::default()
            })
            .await?;
        let read = batch.len();
        for a in batch {
            scan.saw(&a.payload);
            after = Some(a.id);
            let seen = objects.entry(a.object).or_insert_with(|| Seen {
                origin: a.actor.clone(),
                last: a.id,
                last_kind: a.kind,
            });
            seen.last = a.id;
            seen.last_kind = a.kind;
        }
        if read < asked {
            break;
        }
    }
    for (object, seen) in objects {
        if seen.last_kind == ActivityKind::Delete && marks.find(object).await?.is_none() {
            let removal = Removal {
                reach: Reach::UpTo(seen.last),
                by: String::new(),
                origin: seen.origin,
                holds: false,
            };
            marks.record(object, &removal).await?;
        }
    }
    Ok(())
}
