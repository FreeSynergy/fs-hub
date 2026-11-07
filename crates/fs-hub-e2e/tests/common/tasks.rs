//! The real `fs-tasks-server` (library) over a `TaskRepo`, driven over its socket.
//!
//! Pattern: Adapter (test side) — turns a few calls of the `tasks.capnp` contract into plain Rust.

use std::sync::Arc;

use fs_hub_client::Manifest;
use fs_interfaces::rpc::{read_envelope, write_uuid};
use fs_interfaces::tasks_capnp::{items, tasks};
use fs_objects::Store;
use fs_tasks_core::TaskRepo;
use fs_tasks_server::TasksServer;
use uuid::Uuid;

use super::{Env, ME, Prog, serve_on};

/// Tasks write `Task` and read the cards they could assign to.
pub fn manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.6.0")
        .read(&["Profile", "Event"])
        .write(&["Task"])
        .content(&["application/jscalendar+json"])
        .capabilities(&["task.create"])
}

pub async fn start(env: &Env, name: &'static str) -> Prog<items::Client> {
    let store = env.store(name).await;
    let repo = Arc::new(TaskRepo::new(store.clone() as Arc<dyn Store>, ME));
    let root = TasksServer::new(repo.clone(), repo).into_client();
    let (tasks, serving) = serve_on::<tasks::Client>(env.dir.path(), name, root.client).await;
    let items = tasks
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

fn json(uid: &str, title: &str) -> String {
    format!(
        r#"{{"@type":"Task","version":"2.0","uid":"{uid}","title":"{title}","due":"2026-12-01T10:00:00","timeZone":"Europe/Berlin"}}"#
    )
}

impl Prog<items::Client> {
    pub async fn create(&self, uid: &str, title: &str) -> Uuid {
        let mut req = self.root.create_request();
        req.get().set_jscalendar(json(uid, title).as_str());
        let answer = req.send().promise.await.unwrap();
        let envelope = answer
            .get()
            .unwrap()
            .get_task()
            .unwrap()
            .get_envelope()
            .unwrap();
        read_envelope(envelope).unwrap().id.as_uuid()
    }

    pub async fn change(&self, id: Uuid, uid: &str, title: &str) {
        let mut req = self.root.edit_request();
        write_uuid(req.get().init_id(), Some(id));
        req.get().set_jscalendar(json(uid, title).as_str());
        req.send().promise.await.unwrap();
    }

    pub async fn delete(&self, id: Uuid) {
        let mut req = self.root.delete_request();
        write_uuid(req.get().init_id(), Some(id));
        req.send().promise.await.unwrap();
    }
}
