//! End-to-end: a feed that lags ends with `ended`, and `pull` catches up.
//!
//! Lag needs the hub's thread to be busy while changes are logged; in normal use its feed task keeps
//! up. A proposal engine that holds the thread on request makes that happen on a real hub: real
//! socket, real store, real broadcast channel.

mod common;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use common::{note, notes_manifest};
use fs_hub::{HubCore, ProposalEngine, server};
use fs_hub_client::{ChangeNotice, HubSession, HubStatus, Proposal, connect_at};
use fs_interfaces::rpc::FEED_ENDED_PROBLEM_TYPE;
use fs_objects::traits::{ActivityLog, ObjectQuery};
use tokio::sync::oneshot;

/// More changes than the store's change feed holds (`subscribe_capacity`).
const OVER_THE_FEED: usize = 1100;

struct HoldTheThread {
    entered: Sender<()>,
    release: Mutex<Receiver<()>>,
}

#[async_trait]
impl ProposalEngine for HoldTheThread {
    async fn propose(
        &self,
        _: &dyn ObjectQuery,
        _: &dyn ActivityLog,
    ) -> fs_hub::Result<Vec<Proposal>> {
        self.entered.send(()).unwrap();
        // Blocks the hub's only thread, on purpose.
        self.release.lock().unwrap().recv().unwrap();
        Ok(vec![])
    }
}

fn start_hub(
    data: PathBuf,
    socket: PathBuf,
    engine: Arc<dyn ProposalEngine>,
) -> (Arc<HubCore>, oneshot::Sender<()>) {
    let (ready, core) = channel();
    let (stop, stopped) = oneshot::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        tokio::task::LocalSet::new().block_on(&rt, async move {
            let core = HubCore::open_with(&data, engine).await.unwrap();
            let listener = server::bind_socket(&socket).await.unwrap();
            ready.send(core.clone()).unwrap();
            server::serve(core, listener, socket, async {
                let _ = stopped.await;
            })
            .await;
        });
    });
    (core.recv().unwrap(), stop)
}

async fn session(socket: &std::path::Path, name: &str) -> HubSession {
    match connect_at(socket, notes_manifest(name)).await.unwrap() {
        HubStatus::Connected(s) => s,
        HubStatus::Absent => panic!("hub should be there"),
    }
}

#[tokio::test]
async fn a_lagging_feed_ends_with_ended_and_pull_catches_up() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("run/freesynergy/hub.sock");
    let (entered_tx, entered) = channel();
    let (release, release_rx) = channel();
    let engine = Arc::new(HoldTheThread {
        entered: entered_tx,
        release: Mutex::new(release_rx),
    });
    let (core, _stop) = start_hub(dir.path().join("hub-data"), socket.clone(), engine);

    let reader = session(&socket, "reader").await;
    let mut feed = reader.subscribe().await.unwrap();

    // The hub's thread is held; the log grows past what the change feed keeps.
    let holder = session(&socket, "holder").await;
    let held = tokio::spawn(async move { holder.proposals().await });
    tokio::task::spawn_blocking(move || entered.recv().unwrap())
        .await
        .unwrap();
    let store = core.store();
    for i in 0..OVER_THE_FEED {
        store.put(note(&format!("N{i}"), 8), "wiki").await.unwrap();
    }
    release.send(()).unwrap();
    held.await.unwrap().unwrap();

    let problem = loop {
        let notice = tokio::time::timeout(Duration::from_secs(30), feed.recv())
            .await
            .expect("a feed must not end silently")
            .expect("`ended` comes before the channel closes");
        if let ChangeNotice::Ended { problem } = notice {
            break problem;
        }
    };
    assert_eq!(problem.problem_type, FEED_ENDED_PROBLEM_TYPE);
    assert!(problem.detail.contains("missed"), "{}", problem.detail);

    // The program does what the problem says, on the same connection.
    let mut again = reader.subscribe().await.unwrap();
    let (mut pulled, mut since) = (0, None);
    loop {
        let page = reader.pull(since, 0).await.unwrap();
        pulled += page.activities.len();
        since = page.last;
        if !page.more {
            break;
        }
    }
    assert_eq!(pulled, OVER_THE_FEED, "nothing is lost for `pull`");

    let o = note("after", 8);
    let id = o.envelope.id;
    store.put(o, "wiki").await.unwrap();
    let next = tokio::time::timeout(Duration::from_secs(5), again.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(next, ChangeNotice::Created { id: got, .. } if got == id));
}
