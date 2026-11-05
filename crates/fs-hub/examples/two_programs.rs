//! "Programs share work through the hub": contacts and calendar, each with its own store, one hub.
//!
//! `cargo run -p fs-hub --example two_programs`

use std::sync::Arc;

use fs_calendar_core::{CalendarRepo, Event};
use fs_contacts_core::{Card, ContactRepo};
use fs_hub::{Config, Daemon};
use fs_hub_client::{HubStatus, Manifest, connect_at};
use fs_store::{Query, Store, TursoStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("run/freesynergy/hub.sock");
    let hub = Daemon::spawn(Config::new(&socket, dir.path().join("hub")))?;
    println!("hub listening on {}", socket.display());

    // Program A: contacts, own database. It creates a contact.
    let a_store = Arc::new(TursoStore::open(dir.path().join("contacts.db")).await?);
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let mut ada = Card::individual("Ada Lovelace");
    ada.add_email("ada@example.org");
    contacts.create(ada).await?;

    // Program B: calendar, own database. It creates an event with that e-mail as participant.
    let b_store = Arc::new(TursoStore::open(dir.path().join("calendar.db")).await?);
    let calendar = CalendarRepo::new(b_store.clone(), "calendar");
    let mut event = Event::new("Analytical engine review", "2026-12-01T10:00:00", "Europe/Berlin", "PT1H");
    event.add_invitee("Ada Lovelace", "ada@example.org");
    calendar.create(event).await?;

    let a = connect_at(&socket, Manifest::new("contacts", "0.1.0").read(&["Profile"]).write(&["Profile", "Relationship"])).await?;
    let b = connect_at(&socket, Manifest::new("calendar", "0.1.0").read(&["Profile"]).write(&["Event", "Task"])).await?;
    let (HubStatus::Connected(a), HubStatus::Connected(b)) = (a, b) else { return Err("no hub".into()) };

    println!("contacts sync: {:?}", a.sync_once(a_store.as_ref()).await?);
    println!("calendar sync: {:?}", b.sync_once(b_store.as_ref()).await?);

    let hub_store = hub.core().store();
    println!(
        "hub holds {} Profile(s) and {} Event(s)",
        hub_store.count(&Query::new().of_type("Profile")).await?,
        hub_store.count(&Query::new().of_type("Event")).await?
    );
    let known = b_store.query(&Query::new().of_type("Profile").with_index("email", "ada@example.org")).await?;
    println!("calendar can now see the contact for its participant: {:?}", known.iter().map(|o| &o.envelope.name).collect::<Vec<_>>());
    println!("hub proposals: {} (nothing sensitive is ever proposed)", a.proposals().await?.len());
    for p in a.programs().await? {
        println!("program {} reads {:?} writes {:?}", p.manifest.program, p.manifest.reads, p.manifest.writes);
    }
    drop((a, b));
    hub.stop()?;
    Ok(())
}
