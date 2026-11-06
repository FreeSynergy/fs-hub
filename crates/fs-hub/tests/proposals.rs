//! End-to-end: Same-person proposals skip sensitive data and same-program duplicates.

mod common;

use common::{Env, contacts_manifest, person, session};
use fs_people_core::{ContactRepo, ListFilter};
use fs_store::{Query, Store};

#[tokio::test]
async fn proposals_skip_sensitive_data_and_same_program() {
    let env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let c_store = env.store("pm").await;
    let pm = ContactRepo::new(c_store.clone(), "pm");
    let c = session(&env, contacts_manifest("pm")).await;

    // Same e-mail in two programs: a proposal.
    contacts
        .create(person("Grace Hopper", "grace@example.org"))
        .await
        .unwrap();
    pm.create(person("G. Hopper", "GRACE@example.org"))
        .await
        .unwrap();
    // Same e-mail in two programs, but one card is sensitive (ADR 0029): never proposed, never collected.
    let mut secret = person("Alex Doe", "alex@example.org");
    secret.mark_sensitive(["pd:SexualPreference"]);
    let secret = contacts.create(secret).await.unwrap();
    pm.create(person("A. Doe", "alex@example.org"))
        .await
        .unwrap();
    // Same person twice inside one program: the program's own business, not the hub's.
    contacts
        .create(person("Linus A", "linus@example.org"))
        .await
        .unwrap();
    contacts
        .create(person("Linus B", "linus@example.org"))
        .await
        .unwrap();

    let ra = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!(ra.withheld, 1, "{ra:?}");
    c.sync_once(c_store.as_ref()).await.unwrap();

    let props = a.proposals().await.unwrap();
    assert_eq!(props.len(), 1, "{props:?}");
    assert_eq!(props[0].reasons, ["email:grace@example.org"]);
    assert_ne!(props[0].program_a, props[0].program_b);
    let names = [props[0].name_a.as_str(), props[0].name_b.as_str()];
    assert!(names.contains(&"Grace Hopper") && names.contains(&"G. Hopper"));
    assert!(
        props
            .iter()
            .all(|p| p.a != secret.envelope.id && p.b != secret.envelope.id)
    );

    // The hub holds no sensitive object, and the other program never saw it.
    let hub_store = env.hub().core().store();
    assert!(hub_store.get(secret.envelope.id).await.unwrap().is_none());
    let all = hub_store.query(&Query::new()).await.unwrap();
    assert!(all.iter().all(|o| o.envelope.sensitive.is_empty()));
    a.sync_once(a_store.as_ref()).await.unwrap();
    assert!(c_store.get(secret.envelope.id).await.unwrap().is_none());
    // Pulled cards land in the other program's list as ordinary contacts.
    let in_pm = ContactRepo::new(c_store.clone(), "pm")
        .list(&ListFilter::default())
        .await
        .unwrap();
    assert!(in_pm.iter().any(|o| o.envelope.name == "Grace Hopper"));
}
