//! A real hub daemon on a Unix socket, and real program servers beside it.
//!
//! Pattern: Facade (test fixture) — `Env` hides the hub's directories and the way a program server is
//! put on a socket; `Prog<R>` is one running program (its store, its server's root capability, its hub
//! session). Everything runs inside a `LocalSet`, as the Cap'n Proto servers are not `Send`.
#![allow(dead_code)] // each test file uses a different subset

pub mod calendar;
pub mod people;
pub mod story;
pub mod tasks;

use std::path::Path;
use std::sync::Arc;

use fs_hub::{Config, Daemon};
use fs_hub_client::{HubSession, HubStatus, Manifest, connect_at};
use fs_interfaces::rpc::{bind_socket, serve};
use fs_store::traits::ObjectRepository;
use fs_store::{ObjectId, OpenOptions, StoredObject, TursoStore};
use tokio::sync::oneshot;
use uuid::Uuid;

pub const ME: &str = "https://local.invalid/users/owner";

/// The hub and a directory for everything of the programs.
pub struct Env {
    pub dir: tempfile::TempDir,
    pub hub: Daemon,
}

impl Env {
    pub fn start() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("hub-e2e-")
            .tempdir_in(base())
            .unwrap();
        let config = Config::new(
            dir.path().join("run/freesynergy/hub.sock"),
            dir.path().join("hub-data"),
        );
        let hub = Daemon::spawn(config).unwrap();
        Self { dir, hub }
    }

    pub fn socket(&self) -> std::path::PathBuf {
        self.dir.path().join("run/freesynergy/hub.sock")
    }

    /// A store of a program. Sensitive objects are allowed without encryption, as a program's own
    /// mark must be possible in a test (the servers still refuse to create them).
    pub async fn store(&self, name: &str) -> Arc<TursoStore> {
        let opts = OpenOptions {
            allow_sensitive_unencrypted: true,
            ..Default::default()
        };
        Arc::new(
            TursoStore::open_with(self.dir.path().join(format!("{name}.db")), opts)
                .await
                .unwrap(),
        )
    }

    pub async fn session(&self, manifest: Manifest) -> HubSession {
        match connect_at(&self.socket(), manifest).await.unwrap() {
            HubStatus::Connected(s) => s,
            HubStatus::Absent => panic!("the hub should be there"),
        }
    }
}

/// Test directories live under `.scratch` of the checkout when `FS_HUB_E2E_DIR` says so, else in the
/// system's temporary directory (never a small `/tmp` on the build machine).
fn base() -> std::path::PathBuf {
    std::env::var_os("FS_HUB_E2E_DIR").map_or_else(std::env::temp_dir, Into::into)
}

/// One running program: its server, its store, its place at the hub.
pub struct Prog<R> {
    pub name: &'static str,
    pub store: Arc<TursoStore>,
    pub root: R,
    pub session: HubSession,
    _serving: Serving,
}

/// Keeps a server and its client connection alive; stops the server when dropped.
pub struct Serving {
    _connection: Box<dyn std::any::Any>,
    stop: Option<oneshot::Sender<()>>,
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.stop.take().map(|s| s.send(()));
    }
}

/// Put `root` on a socket of its own under `dir` and connect to it.
pub async fn serve_on<C: capnp::capability::FromClientHook + Clone + 'static>(
    dir: &Path,
    name: &str,
    root: capnp::capability::Client,
) -> (C, Serving) {
    let path = dir.join(format!("{name}.sock"));
    let bound = bind_socket(&path).await.unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    tokio::task::spawn_local(serve(bound, root, async {
        let _ = stopped.await;
    }));
    let connection = fs_interfaces::rpc::connect::<C>(&path).await.unwrap();
    let client = connection.client.clone();
    (
        client,
        Serving {
            _connection: Box::new(connection),
            stop: Some(stop),
        },
    )
}

pub fn id(u: Uuid) -> ObjectId {
    ObjectId::from_uuid(u).unwrap()
}

/// The program marks its own object sensitive, as its person would in the program.
pub async fn mark_sensitive(store: &TursoStore, object: Uuid, who: &str) {
    let mut o: StoredObject = store.get(id(object)).await.unwrap().unwrap();
    o.envelope.sensitive = vec!["pd:HealthRecord".into()];
    store.put(o, who).await.unwrap();
}

/// The program lifts its own object's mark again, as its person would.
pub async fn lift_mark(store: &TursoStore, object: Uuid, who: &str) {
    let mut o: StoredObject = store.get(id(object)).await.unwrap().unwrap();
    o.envelope.sensitive.clear();
    store.put(o, who).await.unwrap();
}

/// How many objects of `object_type` this store holds.
pub async fn count(store: &TursoStore, object_type: &str) -> usize {
    use fs_store::traits::ObjectQuery;
    store
        .query(&fs_store::Query::new().of_type(object_type))
        .await
        .unwrap()
        .len()
}

/// The object as this store holds it.
pub async fn held(store: &TursoStore, object: Uuid) -> Option<StoredObject> {
    store.get(id(object)).await.unwrap()
}

/// Two rounds so that everything every program wrote has arrived everywhere.
pub async fn settle(all: &[(&HubSession, &TursoStore)]) {
    for _ in 0..2 {
        for (session, store) in all {
            session.sync_once(*store).await.unwrap();
        }
    }
}
