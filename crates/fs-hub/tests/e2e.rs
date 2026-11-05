//! End-to-end: a hub in a temp dir, two real program cores (contacts, calendar), each with its own store.

use std::sync::Arc;
use std::time::Duration;

use fs_calendar_core::{CalendarRepo, Event};
use fs_contacts_core::{Card, ContactRepo, ListFilter};
use fs_hub::{Config, Daemon};
use fs_hub_client::{ChangeNotice, HubSession, HubStatus, Manifest, connect_at, offer_install};
use fs_store::{ActivityFilter, OpenOptions, Query, Store, TursoStore};

struct Env {
    dir: tempfile::TempDir,
    hub: Option<Daemon>,
}

impl Env {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let hub = Daemon::spawn(config(&dir)).unwrap();
        Self { dir, hub: Some(hub) }
    }
    fn socket(&self) -> std::path::PathBuf {
        self.dir.path().join("run/freesynergy/hub.sock")
    }
    fn hub(&self) -> &Daemon {
        self.hub.as_ref().unwrap()
    }
    async fn store(&self, name: &str) -> Arc<TursoStore> {
        let opts = OpenOptions { allow_sensitive_unencrypted: true, ..Default::default() };
        Arc::new(TursoStore::open_with(self.dir.path().join(format!("{name}.db")), opts).await.unwrap())
    }
}

fn config(dir: &tempfile::TempDir) -> Config {
    Config::new(dir.path().join("run/freesynergy/hub.sock"), dir.path().join("hub-data"))
}

fn contacts_manifest(name: &str) -> Manifest {
    Manifest::new(name, "0.1.0")
        .read(&["Profile"])
        .write(&["Profile", "Relationship"])
        .content(&["application/jscontact+json"])
        .capabilities(&["contact.create", "contact.search"])
}

fn calendar_manifest() -> Manifest {
    Manifest::new("calendar", "0.1.0")
        .read(&["Profile"])
        .write(&["Event", "Task"])
        .content(&["application/jscalendar+json"])
        .capabilities(&["event.create"])
}

async fn session(env: &Env, m: Manifest) -> HubSession {
    match connect_at(&env.socket(), m).await.unwrap() {
        HubStatus::Connected(s) => s,
        HubStatus::Absent => panic!("hub should be there"),
    }
}

fn person(name: &str, email: &str) -> Card {
    let mut c = Card::individual(name);
    c.add_email(email);
    c
}

#[tokio::test]
async fn programs_share_work_through_the_hub() {
    let env = Env::start();

    // Program A: contacts, own store.
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let ada = contacts.create(person("Ada Lovelace", "Ada@Example.org")).await.unwrap();

    // Program B: calendar, own store; invites Ada by e-mail.
    let b_store = env.store("calendar").await;
    let calendar = CalendarRepo::new(b_store.clone(), "calendar");
    let b = session(&env, calendar_manifest()).await;
    let mut ev = Event::new("Analytical engine review", "2026-12-01T10:00:00", "Europe/Berlin", "PT1H");
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
    let profiles = hub_store.query(&Query::new().of_type("Profile")).await.unwrap();
    let events = hub_store.query(&Query::new().of_type("Event")).await.unwrap();
    assert_eq!(profiles.len(), 1);
    assert_eq!(events.len(), 1);
    assert_eq!(profiles[0].envelope.id, ada.envelope.id);
    assert_eq!(events[0].envelope.id, event.envelope.id);

    // B has the contact in its own store, found by the participant's e-mail.
    let found = b_store
        .query(&Query::new().of_type("Profile").with_index("email", "ada@example.org"))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].envelope.name, "Ada Lovelace");
    assert_eq!(found[0].content, ada.content, "JSContact arrives unchanged");

    // What B pulled does not travel back as new work, and A does not receive B's events
    // (A declared only Profile).
    let unsynced = b_store.activities(&ActivityFilter { unsynced_only: true, ..Default::default() }).await.unwrap();
    assert!(unsynced.is_empty(), "{unsynced:?}");
    let ra2 = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!((ra2.pushed, ra2.pulled), (0, 0), "{ra2:?}");
    assert!(a_store.query(&Query::new().of_type("Event")).await.unwrap().is_empty());
    let hub_acts = hub_store.activities(&ActivityFilter::default()).await.unwrap().len();
    b.sync_once(b_store.as_ref()).await.unwrap();
    assert_eq!(hub_store.activities(&ActivityFilter::default()).await.unwrap().len(), hub_acts, "no echo");

    // The hub knows both programs, with their manifests.
    let programs = a.programs().await.unwrap();
    let names: Vec<_> = programs.iter().map(|p| p.manifest.program.as_str()).collect();
    assert_eq!(names, ["calendar", "contacts"]);
    assert!(programs.iter().all(|p| p.connected && p.last_accepted.is_some()));
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
        .query(&Query::new().of_type("Profile").with_index("email", "ada@analytical.example"))
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
    contacts.create(person("Grace Hopper", "grace@example.org")).await.unwrap();
    pm.create(person("G. Hopper", "GRACE@example.org")).await.unwrap();
    // Same e-mail in two programs, but one card is sensitive (ADR 0029): never proposed, never collected.
    let mut secret = person("Alex Doe", "alex@example.org");
    secret.mark_sensitive(["pd:SexualPreference"]);
    let secret = contacts.create(secret).await.unwrap();
    pm.create(person("A. Doe", "alex@example.org")).await.unwrap();
    // Same person twice inside one program: the program's own business, not the hub's.
    contacts.create(person("Linus A", "linus@example.org")).await.unwrap();
    contacts.create(person("Linus B", "linus@example.org")).await.unwrap();

    let ra = a.sync_once(a_store.as_ref()).await.unwrap();
    assert_eq!(ra.withheld, 1, "{ra:?}");
    c.sync_once(c_store.as_ref()).await.unwrap();

    let props = a.proposals().await.unwrap();
    assert_eq!(props.len(), 1, "{props:?}");
    assert_eq!(props[0].reasons, ["email:grace@example.org"]);
    assert_ne!(props[0].program_a, props[0].program_b);
    let names = [props[0].name_a.as_str(), props[0].name_b.as_str()];
    assert!(names.contains(&"Grace Hopper") && names.contains(&"G. Hopper"));
    assert!(props.iter().all(|p| p.a != secret.envelope.id && p.b != secret.envelope.id));

    // The hub holds no sensitive object, and the other program never saw it.
    let hub_store = env.hub().core().store();
    assert!(hub_store.get(secret.envelope.id).await.unwrap().is_none());
    let all = hub_store.query(&Query::new()).await.unwrap();
    assert!(all.iter().all(|o| o.envelope.sensitive.is_empty()));
    a.sync_once(a_store.as_ref()).await.unwrap();
    assert!(c_store.get(secret.envelope.id).await.unwrap().is_none());
    // Pulled cards land in the other program's list as ordinary contacts.
    let in_pm = ContactRepo::new(c_store.clone(), "pm").list(&ListFilter::default()).await.unwrap();
    assert!(in_pm.iter().any(|o| o.envelope.name == "Grace Hopper"));
}

#[tokio::test]
async fn push_is_idempotent_and_state_survives_a_restart() {
    let mut env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    contacts.create(person("Ada", "ada@example.org")).await.unwrap();
    contacts.create(person("Bob", "bob@example.org")).await.unwrap();

    let acts = a_store.activities(&ActivityFilter::default()).await.unwrap();
    let first = a.push(acts.clone()).await.unwrap();
    assert_eq!((first.accepted, first.skipped), (2, 0));
    let second = a.push(acts.clone()).await.unwrap();
    assert_eq!((second.accepted, second.skipped), (0, 2), "same ids twice change nothing");
    assert_eq!(second.last_accepted, first.last_accepted);
    let n = env.hub().core().store().activities(&ActivityFilter::default()).await.unwrap().len();
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
    let again = a.push(a_store.activities(&ActivityFilter::default()).await.unwrap()).await.unwrap();
    assert_eq!((again.accepted, again.skipped), (0, 2));
    assert!(a.programs().await.unwrap().iter().any(|p| p.manifest.program == "contacts" && p.last_accepted.is_some()));
}

#[tokio::test]
async fn pull_cursor_is_kept_by_the_hub() {
    let env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let b_store = env.store("calendar").await;
    let b = session(&env, calendar_manifest()).await;
    for i in 0..3 {
        contacts.create(person(&format!("P{i}"), &format!("p{i}@example.org"))).await.unwrap();
    }
    a.sync_once(a_store.as_ref()).await.unwrap();

    let batch = b.pull(None, 2).await.unwrap();
    assert_eq!(batch.activities.len(), 2);
    assert!(batch.more);
    // Not acknowledged: the same again.
    let same = b.pull(None, 2).await.unwrap();
    assert_eq!(same.activities.first().map(|a| a.id), batch.activities.first().map(|a| a.id));
    b.ack(batch.last.unwrap()).await.unwrap();
    let rest = b.pull(None, 10).await.unwrap();
    assert_eq!(rest.activities.len(), 1);
    assert!(!rest.more);
    // A full sync catches up with what is left.
    let r = b.sync_once(b_store.as_ref()).await.unwrap();
    assert_eq!(r.pulled, 1);
}

#[tokio::test]
async fn subscribers_hear_about_other_programs() {
    let env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let b = session(&env, calendar_manifest()).await;
    let mut rx = b.subscribe().await.unwrap();
    let ada = contacts.create(person("Ada", "ada@example.org")).await.unwrap();
    a.sync_once(a_store.as_ref()).await.unwrap();
    let notice = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.expect("notice in time").unwrap();
    assert_eq!(
        notice,
        ChangeNotice::Created { id: ada.envelope.id, object_type: "Profile".into(), name: "Ada".into() }
    );
}

#[tokio::test]
async fn without_a_hub_programs_keep_working() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(TursoStore::open(dir.path().join("c.db")).await.unwrap());
    let contacts = ContactRepo::new(store.clone(), "contacts");
    let socket = dir.path().join("hub.sock");

    // No socket at all.
    let status = connect_at(&socket, contacts_manifest("contacts")).await.unwrap();
    assert!(matches!(status, HubStatus::Absent));
    // The program works and keeps its log for later.
    contacts.create(person("Ada", "ada@example.org")).await.unwrap();
    assert!(status.sync_once(store.as_ref()).await.unwrap().is_none());
    let unsynced = store.activities(&ActivityFilter { unsynced_only: true, ..Default::default() }).await.unwrap();
    assert_eq!(unsynced.len(), 1);
    assert!(offer_install().contains("hub"));

    // A stale socket file (nobody listening) is Absent too.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists());
    assert!(matches!(connect_at(&socket, contacts_manifest("contacts")).await.unwrap(), HubStatus::Absent));

    // The hub shows up later: the program hands over its log.
    let hub = Daemon::spawn(Config::new(&socket, dir.path().join("hub"))).unwrap();
    let HubStatus::Connected(s) = connect_at(&socket, contacts_manifest("contacts")).await.unwrap() else {
        panic!("hub is running")
    };
    let r = s.sync_once(store.as_ref()).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(hub.core().store().count(&Query::new().of_type("Profile")).await.unwrap(), 1);
}

#[tokio::test]
async fn a_second_hub_on_the_same_socket_is_refused_and_bad_manifests_too() {
    let env = Env::start();
    let other = tempfile::tempdir().unwrap();
    let err = Daemon::spawn(Config::new(env.socket(), other.path().join("d"))).err().expect("must refuse");
    assert!(err.to_string().contains("already running"), "{err}");

    let bad = Manifest::new("Bad Name!", "1");
    assert!(connect_at(&env.socket(), bad).await.is_err());
    let mut old = contacts_manifest("x");
    old.contract_version = "9.9".into();
    assert!(connect_at(&env.socket(), old).await.is_err());
}
