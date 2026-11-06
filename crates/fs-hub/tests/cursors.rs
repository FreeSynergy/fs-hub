//! End-to-end: The pull cursor is kept by the hub and moves only on `ack`.

mod common;

use common::{Env, calendar_manifest, contacts_manifest, person, session};
use fs_contacts_core::ContactRepo;

#[tokio::test]
async fn pull_cursor_is_kept_by_the_hub() {
    let env = Env::start();
    let a_store = env.store("contacts").await;
    let contacts = ContactRepo::new(a_store.clone(), "contacts");
    let a = session(&env, contacts_manifest("contacts")).await;
    let b_store = env.store("calendar").await;
    let b = session(&env, calendar_manifest()).await;
    for i in 0..3 {
        contacts
            .create(person(&format!("P{i}"), &format!("p{i}@example.org")))
            .await
            .unwrap();
    }
    a.sync_once(a_store.as_ref()).await.unwrap();

    let batch = b.pull(None, 2).await.unwrap();
    assert_eq!(batch.activities.len(), 2);
    assert!(batch.more);
    // Not acknowledged: the same again.
    let same = b.pull(None, 2).await.unwrap();
    assert_eq!(
        same.activities.first().map(|a| a.id),
        batch.activities.first().map(|a| a.id)
    );
    b.ack(batch.last.unwrap()).await.unwrap();
    let rest = b.pull(None, 10).await.unwrap();
    assert_eq!(rest.activities.len(), 1);
    assert!(!rest.more);
    // A full sync catches up with what is left.
    let r = b.sync_once(b_store.as_ref()).await.unwrap();
    assert_eq!(r.pulled, 1);
}
