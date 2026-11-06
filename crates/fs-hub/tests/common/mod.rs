//! Shared test environment: a hub in a temp dir, sessions and stores for two program cores.
#![allow(dead_code)] // every test file uses a different subset of these helpers

pub mod raw;

use std::sync::Arc;

use fs_hub::{Config, Daemon};
use fs_hub_client::{HubSession, HubStatus, Manifest, connect_at};
use fs_people_core::Card;
use fs_store::{Envelope, OpenOptions, StoredObject, TursoStore};

pub struct Env {
    pub dir: tempfile::TempDir,
    pub hub: Option<Daemon>,
}

impl Env {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let hub = Daemon::spawn(config(&dir)).unwrap();
        Self {
            dir,
            hub: Some(hub),
        }
    }
    pub fn socket(&self) -> std::path::PathBuf {
        self.dir.path().join("run/freesynergy/hub.sock")
    }
    pub fn hub(&self) -> &Daemon {
        self.hub.as_ref().unwrap()
    }
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
}

pub fn config(dir: &tempfile::TempDir) -> Config {
    Config::new(
        dir.path().join("run/freesynergy/hub.sock"),
        dir.path().join("hub-data"),
    )
}

pub fn contacts_manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.1.0")
        .read(&["Profile"])
        .write(&["Profile", "Relationship"])
        .content(&["application/jscontact+json"])
        .capabilities(&["contact.create", "contact.search"])
}

pub fn calendar_manifest() -> Manifest {
    Manifest::new("calendar", "0.1.0")
        .read(&["Profile"])
        .write(&["Event", "Task"])
        .content(&["application/jscalendar+json"])
        .capabilities(&["event.create"])
}

pub async fn session(env: &Env, m: Manifest) -> HubSession {
    match connect_at(&env.socket(), m).await.unwrap() {
        HubStatus::Connected(s) => s,
        HubStatus::Absent => panic!("hub should be there"),
    }
}

pub fn notes_manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.1.0")
        .read(&["Note"])
        .write(&["Note"])
}

/// A `Note` whose JSON content holds `bytes` of text.
pub fn note(name: &str, bytes: usize) -> StoredObject {
    let content = serde_json::json!({ "text": "x".repeat(bytes) });
    StoredObject::new(Envelope::new("Note", name), "application/json", content)
}

pub fn person(name: &str, email: &str) -> Card {
    let mut c = Card::individual(name);
    c.add_email(email);
    c
}
