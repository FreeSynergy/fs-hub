mod common;
use common::{Env, calendar, people, tasks};

#[tokio::test]
async fn all_start() {
    tokio::task::LocalSet::new().run_until(async {
        let env = Env::start();
        let p = people::start(&env, "people").await;
        let t = tasks::start(&env, "tasks").await;
        let c = calendar::start(&env, "calendar").await;
        p.create("u1", "Ada", "ada@example.org", "+49 30 1234567").await;
        t.create("t1", "Minutes").await;
        c.create("e1", "Meeting", "2026-12-01T10:00:00").await;
    }).await;
}
