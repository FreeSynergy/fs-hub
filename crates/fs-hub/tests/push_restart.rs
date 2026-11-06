//! End-to-end: Idempotent push, declared `writes`, and hub restart.

mod common;

use common::{Env, config, contacts_manifest, person, session};
use fs_hub::Daemon;
use fs_hub_client::Manifest;
use fs_people_core::ContactRepo;
use fs_store::{ActivityFilter, Store};

#[tokio::test]
async fn push_is_idempotent_and_state_survives_a_restart() {
    let mut env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    contacts
        .create(person("Ada", "ada@example.org"))
        .await
        .unwrap();
    contacts
        .create(person("Bob", "bob@example.org"))
        .await
        .unwrap();

    let acts = a_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap();
    let first = a.push(acts.clone()).await.unwrap();
    assert_eq!((first.accepted, first.skipped), (2, 0));
    let second = a.push(acts.clone()).await.unwrap();
    assert_eq!(
        (second.accepted, second.skipped),
        (0, 2),
        "same ids twice change nothing"
    );
    assert_eq!(second.last_accepted, first.last_accepted);
    let n = env
        .hub()
        .core()
        .store()
        .activities(&ActivityFilter::default())
        .await
        .unwrap()
        .len();
    assert_eq!(n, 2);

    // Undeclared type: not collected.
    let mut other = Manifest::new("notes", "0.1.0").write(&["Note"]);
    other.contract_version = fs_hub_proto::CONTRACT_VERSION.into();
    let notes = session(&env, other).await;
    let outcome = notes.push(acts).await.unwrap();
    assert_eq!((outcome.accepted, outcome.withheld), (0, 2));

    // Restart the hub: cursors persist, a repeated push is still skipped.
    drop(a);
    drop(notes);
    env.hub.take().unwrap().stop().unwrap();
    assert!(!env.socket().exists(), "socket removed on shutdown");
    env.hub = Some(Daemon::spawn(config(&env.dir)).unwrap());
    let a = session(&env, contacts_manifest("contacts")).await;
    let again = a
        .push(
            a_store
                .activities(&ActivityFilter::default())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!((again.accepted, again.skipped), (0, 2));
    assert!(
        a.programs()
            .await
            .unwrap()
            .iter()
            .any(|p| p.manifest.program == "contacts" && p.last_accepted.is_some())
    );
}
