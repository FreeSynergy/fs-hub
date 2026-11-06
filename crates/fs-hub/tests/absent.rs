//! End-to-end: Programs keep working without a hub and hand over their log when one appears.

mod common;

use common::{contacts_manifest, person};
use fs_hub::{Config, Daemon};
use fs_hub_client::{HubStatus, connect_at, offer_install};
use fs_people_core::ContactRepo;
use fs_store::{ActivityFilter, Query, Store, TursoStore};
use std::sync::Arc;

#[tokio::test]
async fn without_a_hub_programs_keep_working() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TursoStore::open(dir.path().join("c.db")).await.unwrap());
    let contacts = ContactRepo::new(store.clone(), "contacts");
    let socket = dir.path().join("hub.sock");

    // No socket at all.
    let status = connect_at(&socket, contacts_manifest("contacts"))
        .await
        .unwrap();
    assert!(matches!(status, HubStatus::Absent));
    // The program works and keeps its log for later.
    contacts
        .create(person("Ada", "ada@example.org"))
        .await
        .unwrap();
    assert!(status.sync_once(store.as_ref()).await.unwrap().is_none());
    let unsynced = store
        .activities(&ActivityFilter {
            unsynced_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(unsynced.len(), 1);
    assert!(offer_install().contains("hub"));

    // A stale socket file (nobody listening) is Absent too.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    assert!(matches!(
        connect_at(&socket, contacts_manifest("contacts"))
            .await
            .unwrap(),
        HubStatus::Absent
    ));

    // The hub shows up later: the program hands over its log.
    let hub = Daemon::spawn(Config::new(&socket, dir.path().join("hub"))).unwrap();
    let HubStatus::Connected(s) = connect_at(&socket, contacts_manifest("contacts"))
        .await
        .unwrap()
    else {
        panic!("hub is running")
    };
    let r = s.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(
        hub.core()
            .store()
            .count(&Query::new().of_type("Profile"))
            .await
            .unwrap(),
        1
    );
}
