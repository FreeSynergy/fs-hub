//! End-to-end: how a feed ends when the consumer is the reason (lag: `lag.rs`; a consumer that falls
//! too far behind: `rpc_feed.rs` in `fs-interfaces`).

mod common;

use std::rc::Rc;
use std::time::Duration;

use capnp_rpc::new_client;
use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_hub_client::Problem;
use fs_hub_proto::common_capnp::change_listener;
use fs_hub_proto::read_problem;
use fs_store::traits::{ActivityLog, ObjectRepository};
use fs_store::{Activity, ActivityFilter};
use tokio::sync::mpsc;

async fn log_of_notes(env: &Env, n: usize) -> Vec<Activity> {
    let store = env.store("wiki").await;
    for i in 0..n {
        store.put(note(&format!("N{i}"), 8), "wiki").await.unwrap();
    }
    store.activities(&ActivityFilter::default()).await.unwrap()
}

/// A consumer as a test wants it; `ended` is what it reports.
struct Consumer {
    ended: mpsc::UnboundedSender<Problem>,
    fails: bool,
}

impl change_listener::Server for Consumer {
    async fn change(self: Rc<Self>, _: change_listener::ChangeParams) -> Result<(), capnp::Error> {
        if self.fails {
            return Err(capnp::Error::failed("this listener fails".into()));
        }
        Ok(())
    }

    async fn ended(
        self: Rc<Self>,
        params: change_listener::EndedParams,
        _: change_listener::EndedResults,
    ) -> Result<(), capnp::Error> {
        let problem = read_problem(params.get()?.get_problem()?).unwrap();
        let _ = self.ended.send(problem);
        Ok(())
    }
}

fn consumer(fails: bool) -> (change_listener::Client, mpsc::UnboundedReceiver<Problem>) {
    let (ended, rx) = mpsc::unbounded_channel();
    (new_client(Consumer { ended, fails }), rx)
}

/// A failed stream stays failed: capnp-rpc refuses every later call on that capability, `ended` too.
/// So this consumer cannot be told. What the hub can do: end the feed and let go of the listener,
/// which the consumer sees as its capability being released, and keep the session.
#[tokio::test]
async fn a_failing_listener_is_released_and_the_session_stays() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let acts = log_of_notes(&env, 3).await;
            let wiki = session(&env, notes_manifest("wiki")).await;
            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let (listener, mut ended) = consumer(true);
            let mut request = raw.session.subscribe_request();
            request.get().set_listener(listener);
            let _subscription = request.send().promise.await.unwrap();

            wiki.push(acts).await.unwrap();
            // A stream shows its failure on the send after it: changes keep coming in real life.
            let hub_store = env.hub().core().store();
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    hub_store.put(note("more", 8), "wiki").await.unwrap();
                    tokio::select! {
                        _ = ended.recv() => break,
                        () = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }
                }
            })
            .await
            .expect("the hub lets go of a failed listener");
            assert!(raw.pull(None, 0, true).await.activities.len() >= 3);
        })
        .await;
}

#[tokio::test]
async fn dropping_the_subscription_ends_it_without_ended() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let acts = log_of_notes(&env, 3).await;
            let wiki = session(&env, notes_manifest("wiki")).await;
            let raw = register(&env.socket(), &notes_manifest("raw")).await;
            let (listener, mut ended) = consumer(false);
            let mut request = raw.session.subscribe_request();
            request.get().set_listener(listener);
            drop(request.send().promise.await.unwrap());
            // The hub sees the release on its own turn; the program asked for this, nothing to tell.
            raw.pull(None, 0, true).await;
            wiki.push(acts).await.unwrap();
            let heard = tokio::time::timeout(Duration::from_millis(500), ended.recv()).await;
            assert!(!matches!(heard, Ok(Some(_))), "{heard:?}");
        })
        .await;
}
