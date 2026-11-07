//! Same-person proposals between the cards of two real people servers.

mod common;

use common::{Env, mark_sensitive, people, settle, tasks};

#[tokio::test]
async fn cards_of_two_people_programs_are_proposed_and_marked_ones_are_not() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let a = people::start(&env, "people").await;
            let b = people::start(&env, "contacts").await;
            let t = tasks::start(&env, "tasks").await;
            let all = [
                (&a.session, a.store.as_ref()),
                (&b.session, b.store.as_ref()),
                (&t.session, t.store.as_ref()),
            ];
            // The same e-mail, and the same phone under another e-mail.
            a.create("a1", "Grace Hopper", "grace@example.org", "+1 202 555 0100")
                .await;
            b.create("b1", "G. Hopper", "GRACE@example.org", "+1 202 555 0111")
                .await;
            a.create("a2", "Linus", "linus@a.example", "+49 30 1112223")
                .await;
            b.create("b2", "L. Torvalds", "linus@b.example", "+49 (30) 111 22 23")
                .await;
            // The same e-mail, but one card is marked: neither collected nor proposed.
            a.create("a3", "Alex", "alex@example.org", "+49 30 9990001")
                .await;
            let hidden = b
                .create("b3", "A. Doe", "alex@example.org", "+49 30 9990002")
                .await;
            mark_sensitive(&b.store, hidden, "alex").await;
            // The same person twice in one program is that program's business.
            a.create("a4", "Twin A", "twin@example.org", "+49 30 5550001")
                .await;
            a.create("a5", "Twin B", "twin@example.org", "+49 30 5550002")
                .await;
            settle(&all).await;

            // Any reader of cards is told, and a program that reads no card is not.
            let proposals = t.session.proposals().await.unwrap();
            assert_eq!(proposals.len(), 2, "{proposals:?}");
            let reasons: Vec<_> = proposals.iter().flat_map(|p| p.reasons.clone()).collect();
            assert!(
                reasons.contains(&"email:grace@example.org".to_string()),
                "{reasons:?}"
            );
            assert!(
                reasons.iter().any(|r| r.starts_with("phone:")),
                "{reasons:?}"
            );
            assert!(proposals.iter().all(|p| p.program_a != p.program_b));
            assert!(
                proposals
                    .iter()
                    .all(|p| p.name_a != "A. Doe" && p.name_b != "A. Doe")
            );
            let named = |n: &str| proposals.iter().any(|p| p.name_a == n || p.name_b == n);
            assert!(
                !named("Alex") && !named("Twin A"),
                "marked and same-program cards"
            );
        })
        .await;
}
