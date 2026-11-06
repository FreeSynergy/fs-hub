//! End-to-end: Programs share work through the hub (contacts -> calendar), updates and deletes.

mod common;

use common::{Env, calendar_manifest, contacts_manifest, person, session};
use fs_calendar_core::{CalendarRepo, Event};
use fs_people_core::PeopleRepo;
use fs_store::{ActivityFilter, Query, Store};

#[tokio::test]
async fn programs_share_work_through_the_hub() {
    let env = Env::start();

    // Program A: contacts, own store.
    let a_store = env.store("contacts").await;
    let contacts = PeopleRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let ada = contacts
        .create(person("Ada Lovelace", "Ada@Example.org"))
        .await
        .unwrap();

    // Program B: calendar, own store; invites Ada by e-mail.
    let b_store = env.store("calendar").await;
    let calendar = CalendarRepo::new(b_store.clone(), "calendar");
    let b = session(&env, calendar_manifest()).await;
    let mut ev = Event::new(
        "Analytical engine review",
        "2026-12-01T10:00:00",
        "Europe/Berlin",
        "PT1H",
    );
    ev.add_invitee("Ada Lovelace", "ada@example.org");
    let event = calendar.create(ev).await.unwrap();

    // Both sync. A first: the contact reaches the hub.
    let ra = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!((ra.pushed, ra.accepted, ra.pulled), (1, 1, 0), "{ra:?}");
    let rb = b.sync_once(b_store.as_ref()).await.unwrap();
    assert!(rb.pushed >= 1 && rb.accepted >= 1, "{rb:?}");
    assert_eq!(rb.pulled, 1, "B pulls exactly the contact: {rb:?}");
    assert_eq!(rb.applied, 1);

    // The hub sees both.
    let hub_store = env.hub().core().store();
    let profiles = hub_store
        .query(&Query::new().of_type("Profile"))
        .await
        .unwrap();
    let events = hub_store
        .query(&Query::new().of_type("Event"))
        .await
        .unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(events.len(), 1);
    assert_eq!(profiles[0].envelope.id, ada.envelope.id);
    assert_eq!(events[0].envelope.id, event.envelope.id);

    // B has the contact in its own store, found by the participant's e-mail.
    let found = b_store
        .query(
            &Query::new()
                .of_type("Profile")
                .with_index("email", "ada@example.org"),
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].envelope.name, "Ada Lovelace");
    assert_eq!(found[0].content, ada.content, "JSContact arrives unchanged");

    // What B pulled does not travel back as new work, and A does not receive B's events
    // (A declared only Profile).
    let unsynced = b_store
        .activities(&ActivityFilter {
            unsynced_only: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(unsynced.is_empty(), "{unsynced:?}");
    let ra2 = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!((ra2.pushed, ra2.pulled), (0, 0), "{ra2:?}");
    assert!(
        a_store
            .query(&Query::new().of_type("Event"))
            .await
            .unwrap()
            .is_empty()
    );
    let hub_acts = hub_store
        .activities(&ActivityFilter::default())
        .await
        .unwrap()
        .len();
    b.sync_once(b_store.as_ref()).await.unwrap();
    assert_eq!(
        hub_store
            .activities(&ActivityFilter::default())
            .await
            .unwrap()
            .len(),
        hub_acts,
        "no echo"
    );

    // The hub knows both programs, with their manifests.
    let programs = a.programs().await.unwrap();
    let names: Vec<_> = programs
        .iter()
        .map(|p| p.manifest.program.as_str())
        .collect();
    assert_eq!(names, ["calendar", "contacts"]);
    assert!(
        programs
            .iter()
            .all(|p| p.connected && p.last_accepted.is_some())
    );
    assert_eq!(programs[1].manifest.reads, ["Profile"]);

    // Nothing to propose (one contact, one event), and nothing sensitive.
    assert!(a.proposals().await.unwrap().is_empty());

    // Updates and deletes travel too.
    let mut card = contacts.get_card(ada.envelope.id).await.unwrap().unwrap();
    card.add_email("ada@analytical.example");
    contacts.update(ada.envelope.id, card).await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();
    let again = b_store
        .query(
            &Query::new()
                .of_type("Profile")
                .with_index("email", "ada@analytical.example"),
        )
        .await
        .unwrap();
    assert_eq!(again.len(), 1);
    contacts.delete(ada.envelope.id).await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    b.sync_once(b_store.as_ref()).await.unwrap();
    assert!(b_store.get(ada.envelope.id).await.unwrap().is_none());
    assert!(hub_store.get(ada.envelope.id).await.unwrap().is_none());
    assert!(hub_store.get(event.envelope.id).await.unwrap().is_some());
}
