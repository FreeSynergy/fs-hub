//! The real `fs-calendar-server` (library) over a `CalendarRepo`, driven over its socket.
//!
//! Pattern: Adapter (test side) — turns a few calls of the `events.capnp` contract into plain Rust.

use std::sync::Arc;

use fs_calendar_core::CalendarRepo;
use fs_calendar_server::CalendarServer;
use fs_hub_client::Manifest;
use fs_interfaces::events_capnp::{events, items};
use fs_interfaces::rpc::{read_envelope, write_uuid};
use fs_store::Store;
use uuid::Uuid;

use super::{Env, ME, Prog, serve_on};

/// The calendar writes `Event` and reads cards and tasks.
pub fn manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.5.0")
        .read(&["Profile", "Task"])
        .write(&["Event"])
        .content(&["application/jscalendar+json"])
        .capabilities(&["event.create"])
}

pub async fn start(env: &Env, name: &'static str) -> Prog<items::Client> {
    let store = env.store(name).await;
    let repo = Arc::new(CalendarRepo::new(store.clone() as Arc<dyn Store>, ME));
    let root = CalendarServer::new(repo.clone(), repo).into_client();
    let (events, serving) = serve_on::<events::Client>(env.dir.path(), name, root.client).await;
    let items = events
        .items_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_items()
        .unwrap();
    Prog {
        name,
        store,
        root: items,
        session: env.session(manifest(name)).await,
        _serving: serving,
    }
}

fn json(uid: &str, title: &str, start: &str) -> String {
    format!(
        r#"{{"@type":"Event","version":"2.0","uid":"{uid}","title":"{title}","start":"{start}","timeZone":"Europe/Berlin","duration":"PT1H"}}"#
    )
}

impl Prog<items::Client> {
    pub async fn create(&self, uid: &str, title: &str, start: &str) -> Uuid {
        let mut req = self.root.create_request();
        req.get().set_jscalendar(json(uid, title, start).as_str());
        let answer = req.send().promise.await.unwrap();
        let envelope = answer.get().unwrap().get_event().unwrap().get_envelope().unwrap();
        read_envelope(envelope).unwrap().id.as_uuid()
    }

    pub async fn change(&self, id: Uuid, uid: &str, title: &str, start: &str) {
        let mut req = self.root.update_request();
        write_uuid(req.get().init_id(), Some(id));
        req.get().set_jscalendar(json(uid, title, start).as_str());
        req.send().promise.await.unwrap();
    }

    pub async fn delete(&self, id: Uuid) {
        let mut req = self.root.delete_request();
        write_uuid(req.get().init_id(), Some(id));
        req.send().promise.await.unwrap();
    }
}
