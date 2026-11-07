//! A mark another program set is not lifted by the owner's update without a `before` image (a purge
//! leaves none), but by the marking program; the owner's own data stays untouched either way.
//!
//! Pattern: Scenario test — one story per test, told through the real servers, the hub and the client.

mod common;

use common::story::{Story, card, marked_by_contacts, removal, update_without_before};
use common::{held, id};

#[tokio::test]
async fn the_owner_cannot_lift_a_mark_of_another_program_without_a_before_image() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            marked_by_contacts(&s).await;
            let raw = update_without_before(&card(&s, "Ada Lovelace", false).await);
            let out = s.p.session.push(vec![raw]).await.unwrap();
            assert_eq!(
                (out.accepted, out.withheld),
                (0, 1),
                "held, not taken: {out:?}"
            );
            s.settle().await;
            assert!(
                held(&s.c.store, s.ada).await.is_none(),
                "the reader still has no copy"
            );
            assert!(removal(&s).await.holds, "the mark holds");
            assert_eq!(
                held(&s.p.store, s.ada).await.unwrap().envelope.name,
                "Ada",
                "the owner's data is untouched"
            );
        })
        .await;
}

#[tokio::test]
async fn a_purge_does_not_make_the_owner_lift_another_programs_mark() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            marked_by_contacts(&s).await;
            let hub = &s.p.session;
            hub.sync_then_purge(s.p.store.as_ref(), id(s.ada))
                .await
                .unwrap();
            s.p.change(s.ada, "u1", "Ada Lovelace", "ada@example.org", "+49 30 1")
                .await;
            let report = hub.sync_once(s.p.store.as_ref()).await.unwrap();
            assert_eq!((report.accepted, report.withheld), (0, 1), "{report:?}");
            s.settle().await;
            assert!(held(&s.c.store, s.ada).await.is_none());
            assert!(removal(&s).await.holds);
            assert_eq!(
                held(&s.p.store, s.ada).await.unwrap().envelope.name,
                "Ada Lovelace"
            );
        })
        .await;
}

#[tokio::test]
async fn the_marking_program_lifts_without_a_before_image_but_its_copy_stays_out() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let s = Story::begin().await;
            let contacts = marked_by_contacts(&s).await;
            let out = contacts
                .push(vec![update_without_before(&card(&s, "Ada", false).await)])
                .await
                .unwrap();
            assert_eq!((out.accepted, out.skipped), (0, 1), "lifted only: {out:?}");
            s.settle().await;
            assert!(!removal(&s).await.holds, "the mark is lifted");
            assert!(
                held(&s.c.store, s.ada).await.is_none(),
                "the owner's version is awaited"
            );
        })
        .await;
}
