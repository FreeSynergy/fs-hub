//! A pull does not hold much more than what it answers. Measured on the real binary, over a real
//! socket: `VmHWM` of a freshly started hub, so nothing of filling it counts.

mod common;

use std::path::Path;
use std::process::{Child, Command};
use std::time::Duration;

use common::{note, notes_manifest};
use fs_hub_client::{HubSession, HubStatus, connect_at};
use fs_store::TursoStore;
use fs_store::traits::ObjectRepository;

const MIB: usize = 1 << 20;
const NOTES: usize = 32;
const NOTE_BYTES: usize = 14 * MIB;
/// Two notes make a page of 28 MiB. Reading all 32 entries each time came to about 835 MiB.
const MOST_PEAK_BYTES: u64 = 400 << 20;

fn start(socket: &Path, data: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_fs-hub"))
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .spawn()
        .unwrap()
}

async fn connect(socket: &Path, name: &str) -> HubSession {
    for _ in 0..200 {
        if socket.exists()
            && let HubStatus::Connected(s) = connect_at(socket, notes_manifest(name)).await.unwrap()
        {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the hub did not come up");
}

fn peak_bytes(hub: &Child) -> u64 {
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
    std::fs::create_dir_all(&data).unwrap();

    let mut hub = start(&socket, &data);
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
    hub.kill().unwrap();
    hub.wait().unwrap();

    let mut hub = start(&socket, &data);
    let reader = connect(&socket, "reader").await;
    let page = reader.pull(None, 0).await.unwrap();
    assert_eq!(page.activities.len(), 2);
    assert!(page.more);
    let peak = peak_bytes(&hub);
    eprintln!("MEASURE hub peak: {} MiB", peak >> 20);
    hub.kill().unwrap();
    hub.wait().unwrap();
    assert!(peak < MOST_PEAK_BYTES, "{} MiB", peak >> 20);
}
