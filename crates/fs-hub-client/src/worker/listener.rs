//! Receiving live change notices.
//!
//! Pattern: Observer — the hub calls our `ChangeListener`; every call, also `ended`, becomes a
//! `ChangeNotice` on a channel the program reads.

use std::cell::Cell;
use std::rc::Rc;

use capnp_rpc::new_client;
use fs_hub_proto::common_capnp::{change, change_listener, subscription};
use fs_hub_proto::hub_capnp::session;
use fs_hub_proto::{ChangeNotice, Problem, read_uuid};
use fs_interfaces::rpc::FEED_ENDED_PROBLEM_TYPE;
use fs_store::ObjectId;
use tokio::sync::mpsc;

use crate::Result;

struct Listener {
    tx: mpsc::UnboundedSender<ChangeNotice>,
    ended: Cell<bool>,
}

impl Listener {
    fn new(tx: mpsc::UnboundedSender<ChangeNotice>) -> Self {
        Self {
            tx,
            ended: Cell::new(false),
        }
    }

    /// Nothing follows the first end.
    fn end(&self, problem: Problem) {
        if !self.ended.replace(true) {
            let _ = self.tx.send(ChangeNotice::Ended { problem });
        }
    }
}

fn read_notice(
    params: &change_listener::ChangeParams,
) -> std::result::Result<ChangeNotice, capnp::Error> {
    Ok(match params.get()?.get_change()?.which()? {
        change::Which::Created(e) => {
            let e = e?;
            ChangeNotice::Created {
                id: object_id(e.get_id()?)?,
                object_type: e.get_type()?.to_string()?,
                name: e.get_name()?.to_string()?,
            }
        }
        change::Which::Updated(e) => {
            let e = e?;
            ChangeNotice::Updated {
                id: object_id(e.get_id()?)?,
                object_type: e.get_type()?.to_string()?,
                name: e.get_name()?.to_string()?,
            }
        }
        change::Which::Deleted(u) => ChangeNotice::Deleted { id: object_id(u?)? },
    })
}

impl change_listener::Server for Listener {
    // The generated trait method is async; nothing here awaits. An error answer would fail the stream
    // for good, `ended` included (`common.capnp`), so a change that cannot be read ends the feed here.
    #[allow(clippy::unused_async_trait_impl)]
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> std::result::Result<(), capnp::Error> {
        if self.ended.get() {
            return Ok(());
        }
        match read_notice(&params) {
            Ok(notice) => {
                let _ = self.tx.send(notice);
            }
            Err(e) => self.end(Problem::new(
                FEED_ENDED_PROBLEM_TYPE,
                "The feed ended",
                format!("a change could not be read: {e}"),
                422,
            )),
        }
        Ok(())
    }

    // Nothing here awaits either.
    #[allow(clippy::unused_async_trait_impl)]
    async fn ended(
        self: Rc<Self>,
        params: change_listener::EndedParams,
        _: change_listener::EndedResults,
    ) -> std::result::Result<(), capnp::Error> {
        let problem = Problem::read(params.get()?.get_problem()?)?;
        self.end(problem);
        Ok(())
    }
}

fn object_id(
    u: fs_hub_proto::common_capnp::uuid::Reader<'_>,
) -> std::result::Result<ObjectId, capnp::Error> {
    let u = read_uuid(u).ok_or_else(|| capnp::Error::failed("change without id".into()))?;
    ObjectId::from_uuid(u).map_err(|e| capnp::Error::failed(e.to_string()))
}

pub(super) async fn subscribe(
    s: &session::Client,
    tx: mpsc::UnboundedSender<ChangeNotice>,
) -> Result<subscription::Client> {
    let mut req = s.subscribe_request();
    req.get().set_listener(new_client(Listener::new(tx)));
    let resp = req.send().promise.await?;
    Ok(resp.get()?.get_subscription()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unreadable_change_ends_the_feed_without_failing_the_stream() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let client: change_listener::Client = new_client(Listener::new(tx));
        for _ in 0..2 {
            // No id: it cannot be read. The call still succeeds, the stream must stay healthy.
            let mut request = client.change_request();
            let _ = request.get().init_change().init_created();
            request.send().await.unwrap();
        }
        let first = rx.recv().await.unwrap();
        assert!(
            matches!(&first, ChangeNotice::Ended { problem } if problem.problem_type == FEED_ENDED_PROBLEM_TYPE),
            "{first:?}"
        );
        assert!(rx.try_recv().is_err(), "nothing follows the end");
    }
}
