//! A pull does not hold much more than what it answers. Measured on the real binary, over a real
//! socket: `VmHWM` of a freshly started hub, so nothing of filling it counts.

mod common;

use common::binary::{Hub, connect, kill, start};
use common::note;
use fs_objects::TursoStore;
use fs_objects::traits::ObjectRepository;

const MIB: usize = 1 << 20;
const NOTES: usize = 32;
const NOTE_BYTES: usize = 14 * MIB;
/// Two notes make a page of 28 MiB. Reading all 32 entries each time came to about 835 MiB.
const MOST_PEAK_BYTES: u64 = 400 << 20;

fn peak_bytes(hub: &Hub) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{}/status", hub.id())).unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    let kib: u64 = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    kib * 1024
}

#[tokio::test]
async fn a_page_of_large_entries_does_not_read_the_whole_scan() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("hub.sock");
    let data = dir.path().join("hub-data");

    let hub = start(&socket, &data);
    let writer = connect(&socket, "wiki").await;
    let store = TursoStore::open(dir.path().join("wiki.db")).await.unwrap();
    for i in 0..NOTES {
        store
            .put(note(&format!("N{i}"), NOTE_BYTES), "wiki")
            .await
            .unwrap();
    }
    assert_eq!(
        writer.sync_once(&store).await.unwrap().accepted as usize,
        NOTES
    );
    drop(writer);
    kill(hub);

    let hub = start(&socket, &data);
    let reader = connect(&socket, "reader").await;
    let page = reader.pull(None, 0).await.unwrap();
    assert_eq!(page.activities.len(), 2);
    assert!(page.more);
    let peak = peak_bytes(&hub);
    eprintln!("MEASURE hub peak: {} MiB", peak >> 20);
    kill(hub);
    assert!(peak < MOST_PEAK_BYTES, "{} MiB", peak >> 20);
}
