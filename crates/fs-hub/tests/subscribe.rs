//! End-to-end: Live change notices for subscribers.

mod common;

use common::{Env, calendar_manifest, contacts_manifest, person, session};
use fs_contacts_core::ContactRepo;
use fs_hub_client::ChangeNotice;
use std::time::Duration;

#[tokio::test]
async fn subscribers_hear_about_other_programs() {
    let env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let b = session(&env, calendar_manifest()).await;
    let mut rx = b.subscribe().await.unwrap();
    let ada = contacts
        .create(person("Ada", "ada@example.org"))
        .await
        .unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    let notice = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("notice in time")
        .unwrap();
    assert_eq!(
        notice,
        ChangeNotice::Created {
            id: ada.envelope.id,
            object_type: "Profile".into(),
            name: "Ada".into()
        }
    );
}
