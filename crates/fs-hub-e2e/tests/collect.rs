//! The hub collects from three real program servers, keeps a cursor per program, and carries creates,
//! changes and deletes across.

mod common;

use common::{Env, calendar, count, held, people, settle, tasks};
use fs_hub_client::Manifest;

#[tokio::test]
async fn objects_travel_between_the_programs_and_a_delete_removes_the_copy() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let p = people::start(&env, "people").await;
            let t = tasks::start(&env, "tasks").await;
            let c = calendar::start(&env, "calendar").await;
            // A reader that may read tasks only: it never sees a card.
            let wiki_store = env.store("wiki").await;
            let wiki = env
                .session(Manifest::new("wiki", "0.1.0").read(&["Task"]))
                .await;
            let all = [
                (&p.session, p.store.as_ref()),
                (&t.session, t.store.as_ref()),
                (&c.session, c.store.as_ref()),
                (&wiki, wiki_store.as_ref()),
            ];

            let ada = p
                .create("u1", "Ada", "ada@example.org", "+49 30 1234567")
                .await;
            let minutes = t.create("t1", "Minutes").await;
            let meeting = c.create("e1", "Meeting", "2026-12-01T10:00:00").await;
            settle(&all).await;

            // Each program got what it declared, and only that.
            assert!(held(&c.store, ada).await.is_some(), "calendar reads cards");
            assert!(
                held(&c.store, minutes).await.is_some(),
                "calendar reads tasks"
            );
            assert!(held(&t.store, ada).await.is_some() && held(&t.store, meeting).await.is_some());
            assert_eq!(
                count(&p.store, "Task").await + count(&p.store, "Event").await,
                0
            );
            assert_eq!(
                count(&wiki_store, "Profile").await,
                0,
                "no card for a reader of tasks"
            );
            assert_eq!(count(&wiki_store, "Event").await, 0);
            assert!(held(&wiki_store, minutes).await.is_some());

            // A card reaches a reader with what finds it, and without whom it is about.
            let copy = held(&c.store, ada).await.unwrap();
            assert_eq!(copy.envelope.attributed_to, None, "the person stays home");
            assert_eq!(
                copy.index.get("email").map(Vec::as_slice),
                Some(&["ada@example.org".to_string()][..])
            );
            assert!(copy.index.contains_key("phone"));

            // The hub knows all three writers, each with its own place in the log.
            let infos = p.session.programs().await.unwrap();
            let of = |n: &str| infos.iter().find(|i| i.manifest.program == n).unwrap();
            for n in ["people", "tasks", "calendar"] {
                assert!(of(n).last_accepted.is_some(), "{n} handed something over");
            }
            assert!(of("wiki").last_accepted.is_none(), "wiki wrote nothing");
            let (tasks_cursor, calendar_cursor) =
                (of("tasks").pull_cursor, of("calendar").pull_cursor);
            assert!(tasks_cursor.is_some() && calendar_cursor.is_some());
            assert!(of("wiki").pull_cursor.is_some());

            // A change in one program changes the copies; another program's cursor stays where it was.
            p.change(
                ada,
                "u1",
                "Ada Lovelace",
                "ada@example.org",
                "+49 30 1234567",
            )
            .await;
            t.change(minutes, "t1", "Minutes of March").await;
            c.change(meeting, "e1", "Review", "2026-12-02T10:00:00")
                .await;
            settle(&all).await;
            assert_eq!(
                held(&c.store, ada).await.unwrap().envelope.name,
                "Ada Lovelace"
            );
            assert_eq!(
                held(&c.store, minutes).await.unwrap().envelope.name,
                "Minutes of March"
            );
            assert_eq!(
                held(&t.store, meeting).await.unwrap().envelope.name,
                "Review"
            );
            assert_eq!(
                held(&wiki_store, minutes).await.unwrap().envelope.name,
                "Minutes of March"
            );
            let infos = p.session.programs().await.unwrap();
            let of = |n: &str| infos.iter().find(|i| i.manifest.program == n).unwrap();
            assert!(of("tasks").pull_cursor > tasks_cursor, "moved on");
            assert!(of("calendar").pull_cursor > calendar_cursor, "moved on");

            // A delete removes the copy at the reader, in every program.
            t.delete(minutes).await;
            c.delete(meeting).await;
            p.delete(ada).await;
            settle(&all).await;
            assert!(held(&c.store, minutes).await.is_none());
            assert!(held(&wiki_store, minutes).await.is_none());
            assert!(held(&t.store, meeting).await.is_none());
            assert!(held(&c.store, ada).await.is_none() && held(&t.store, ada).await.is_none());
            assert!(held(&p.store, ada).await.is_none());
        })
        .await;
}
