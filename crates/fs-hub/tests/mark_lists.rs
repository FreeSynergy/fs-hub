//! The lists read only their own kind of entry.

mod common;

use common::note;
use fs_hub::{MarkedRemovals, OriginalTimes, StoreMarked, StoreOriginals};
use fs_objects::TursoStore;
use fs_objects::traits::ObjectRepository;

/// A foreign object at the id of an entry on either list is no entry: reading stays possible.
#[tokio::test]
async fn each_list_reads_only_its_own_kind() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(TursoStore::open(dir.path().join("x.db")).await.unwrap());
    let foreign = note("foreign", 8);
    let id = foreign.envelope.id;
    store.put(foreign, "x").await.unwrap();
    let marks = StoreMarked::new(store.clone()).await.unwrap();
    assert_eq!(marks.find(id).await.unwrap(), None);
    let times = StoreOriginals::new(store).await.unwrap();
    let entry = fs_objects::ActivityId::from_uuid(id.as_uuid()).unwrap();
    assert_eq!(times.find(entry).await.unwrap(), None);
}
