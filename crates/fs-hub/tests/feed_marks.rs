//! End-to-end: a notice waiting in the queue says nothing of an object marked meanwhile.

mod common;

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use capnp_rpc::new_client;
use common::raw::register;
use common::{Env, note, notes_manifest, session};
use fs_hub_proto::common_capnp::{change, change_listener};
use fs_store::traits::ObjectRepository;
use tokio::sync::{Notify, mpsc};

/// A few of these fill the stream's window of 64 KiB.
const LONG_NAME: usize = 40 << 10;
const NOTES: usize = 7;

struct Slow {
    heard: mpsc::UnboundedSender<String>,
    gate: Arc<Notify>,
}

impl change_listener::Server for Slow {
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> Result<(), capnp::Error> {
        self.gate.notified().await;
        self.gate.notify_one();
        let told = match params.get()?.get_change()?.which()? {
            change::Which::Created(e) => e?.get_name()?.to_string()?,
            change::Which::Updated(e) => e?.get_name()?.to_string()?,
            change::Which::Deleted(_) => "deleted".to_string(),
        };
        let _ = self.heard.send(told);
        Ok(())
    }
}

#[tokio::test]
async fn a_queued_notice_does_not_tell_the_name_after_a_mark() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let env = Env::start();
            let store = env.store("owner").await;
            let owner = session(&env, notes_manifest("owner")).await;
            let reader = register(&env.socket(), &notes_manifest("reader")).await;
            let (heard_tx, mut heard) = mpsc::unbounded_channel();
            let gate = Arc::new(Notify::new());
            let mut request = reader.session.subscribe_request();
            request.get().set_listener(new_client(Slow {
                heard: heard_tx,
                gate: gate.clone(),
            }));
            let _subscription = request.send().promise.await.unwrap();

            // The consumer is held: what fits the stream's window is on its way (the window, plus the
            // largest message), the last one waits in the hub's queue.
            let name = |c: usize| format!("{c}").repeat(LONG_NAME);
            let last = note(&name(NOTES), 8);
            for i in 0..NOTES {
                store.put(note(&name(i), 8), "ada").await.unwrap();
            }
            store.put(last.clone(), "ada").await.unwrap();
            owner.sync_once(store.as_ref()).await.unwrap();
            let mut marked = last;
            marked.envelope.sensitive = vec!["pd:Health".into()];
            store.put(marked, "ada").await.unwrap();
            assert_eq!(owner.sync_once(store.as_ref()).await.unwrap().withheld, 1);

            gate.notify_one();
            let mut told = Vec::new();
            while told.last().is_none_or(|t| t != "deleted") {
                let got = tokio::time::timeout(Duration::from_secs(10), heard.recv())
                    .await
                    .expect("the delete comes")
                    .unwrap();
                told.push(got);
            }
            assert!(
                !told.contains(&name(NOTES)),
                "the marked object's name went out"
            );
            assert!(told.contains(&name(0)), "the others are told");
        })
        .await;
}
