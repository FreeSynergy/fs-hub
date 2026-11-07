//! End-to-end: no program gets read access to `Relationship` (role privacy, sprint thread 3 point 5).

mod common;

use common::{Env, contacts_manifest, note, notes_manifest, session};
use fs_hub_client::{Manifest, connect_at};
use fs_store::traits::{ObjectQuery, ObjectRepository};
use fs_store::{Envelope, StoredObject};

fn assignment(name: &str) -> StoredObject {
    let content = serde_json::json!({"type": "Relationship", "relationship": "customer"});
    StoredObject::new(
        Envelope::new("Relationship", name),
        "application/json",
        content,
    )
}

#[tokio::test]
async fn a_manifest_that_reads_relationships_is_refused() {
    let env = Env::start();
    let greedy = Manifest::new("greedy", "0.1.0").read(&["Profile", "Relationship"]);
    let err = connect_at(&env.socket(), greedy)
        .await
        .err()
        .expect("must refuse");
    assert!(err.to_string().contains("Relationship"), "{err}");
    // Writing them stays allowed: `fs-people` hands over its links; others still register.
    assert!(
        connect_at(&env.socket(), contacts_manifest("people"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_role_assignment_reaches_no_reader() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let (people_store, reader_store) =
                (env.store("people").await, env.store("reader").await);
            let people = session(&env, contacts_manifest("people")).await;
            let reader = session(&env, notes_manifest("reader")).await;
            people_store
                .put(assignment("customer of Ada"), "ada")
                .await
                .unwrap();
            people_store.put(note("shown", 8), "ada").await.unwrap();
            people.sync_once(people_store.as_ref()).await.unwrap();
            let report = reader.sync_once(reader_store.as_ref()).await.unwrap();
            assert_eq!(
                report.pulled, 0,
                "the reader reads Note, and nobody wrote one"
            );
            let all = reader_store
                .query(&fs_store::Query::new().of_type("Relationship"))
                .await
                .unwrap();
            assert!(all.is_empty());
        })
        .await;
}
