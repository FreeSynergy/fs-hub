//! The real `fs-people-server` (library) over a `PeopleRepo`, driven over its socket.
//!
//! Pattern: Adapter (test side) — turns a few calls of the `people.capnp` contract into plain Rust.

use std::sync::Arc;

use fs_hub_client::Manifest;
use fs_interfaces::people_capnp::{people, persons};
use fs_interfaces::rpc::{read_envelope, write_uuid};
use fs_people_core::PeopleRepo;
use fs_people_server::PeopleServer;
use fs_objects::Store;
use uuid::Uuid;

use super::{Env, ME, Prog, serve_on};

/// What a people program tells the hub. It hands over its links (`Relationship`), reads no type of
/// another program but cards.
pub fn manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.5.0")
        .read(&["Profile"])
        .write(&["Profile", "Relationship"])
        .content(&["application/jscontact+json"])
        .capabilities(&["person.create"])
}

pub async fn start(env: &Env, name: &'static str) -> Prog<persons::Client> {
    let store = env.store(name).await;
    let repo = Arc::new(PeopleRepo::new(store.clone() as Arc<dyn Store>, ME));
    let root = PeopleServer::new(repo.clone(), repo.clone(), repo).into_client();
    let (people, serving) = serve_on::<people::Client>(env.dir.path(), name, root.client).await;
    let persons = people
        .persons_request()
        .send()
        .promise
        .await
        .unwrap()
        .get()
        .unwrap()
        .get_persons()
        .unwrap();
    Prog {
        name,
        store,
        root: persons,
        session: env.session(manifest(name)).await,
        _serving: serving,
    }
}

fn card(uid: &str, name: &str, email: &str, phone: &str) -> String {
    format!(
        r#"{{"@type":"Card","version":"1.0","uid":"{uid}","name":{{"full":"{name}"}},"emails":{{"e1":{{"address":"{email}"}}}},"phones":{{"p1":{{"number":"{phone}"}}}}}}"#
    )
}

impl Prog<persons::Client> {
    pub async fn create(&self, uid: &str, name: &str, email: &str, phone: &str) -> Uuid {
        let mut req = self.root.create_request();
        req.get()
            .set_jscontact(card(uid, name, email, phone).as_str());
        let answer = req.send().promise.await.unwrap();
        let envelope = answer
            .get()
            .unwrap()
            .get_person()
            .unwrap()
            .get_envelope()
            .unwrap();
        read_envelope(envelope).unwrap().id.as_uuid()
    }

    pub async fn change(&self, id: Uuid, uid: &str, name: &str, email: &str, phone: &str) {
        let mut req = self.root.update_request();
        write_uuid(req.get().init_id(), Some(id));
        req.get()
            .set_jscontact(card(uid, name, email, phone).as_str());
        req.send().promise.await.unwrap();
    }

    pub async fn delete(&self, id: Uuid) {
        let mut req = self.root.delete_request();
        write_uuid(req.get().init_id(), Some(id));
        req.send().promise.await.unwrap();
    }
}
