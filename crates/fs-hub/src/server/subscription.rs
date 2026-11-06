//! Live change notices to a subscribed program.
//!
//! Pattern: Observer — a local task listens to the core's change feed, filters with the
//! visibility rule and calls the program's `ChangeListener`; dropping the `Subscription`
//! capability aborts the task.

use std::cell::RefCell;
use std::sync::Arc;

use fs_hub_proto::common_capnp::{change_listener, subscription};
use fs_hub_proto::{write_envelope, write_uuid};
use fs_store::{Activity, ActivityKind};
use tokio::sync::broadcast::error::RecvError;

use super::errors::rpc_err;
use crate::HubCore;

/// Dropping the capability ends the subscription.
pub(super) struct SubscriptionImpl {
    task: RefCell<Option<tokio::task::JoinHandle<()>>>,
}

impl SubscriptionImpl {
    pub fn new(task: tokio::task::JoinHandle<()>) -> Self {
        Self {
            task: RefCell::new(Some(task)),
        }
    }
}

impl subscription::Server for SubscriptionImpl {}

impl Drop for SubscriptionImpl {
    fn drop(&mut self) {
        if let Some(t) = self.task.borrow_mut().take() {
            t.abort();
        }
    }
}

/// Forward visible activities to `listener` until it goes away.
pub(super) fn spawn_notifier(
    core: Arc<HubCore>,
    program: String,
    reads: Vec<String>,
    listener: change_listener::Client,
) -> tokio::task::JoinHandle<()> {
    let mut rx = core.changes();
    tokio::task::spawn_local(async move {
        loop {
            match rx.recv().await {
                Ok(a) => {
                    if !core.visible_to(&program, &reads, &a).await {
                        continue;
                    }
                    if notify(&listener, &a).await.is_err() {
                        break; // listener gone
                    }
                }
                Err(RecvError::Lagged(_)) => {} // the program catches up with `pull`
                Err(RecvError::Closed) => break,
            }
        }
    })
}

async fn notify(listener: &change_listener::Client, a: &Activity) -> Result<(), capnp::Error> {
    let cs = a.change_set().map_err(rpc_err)?;
    let mut req = listener.change_request();
    {
        let change = req.get().init_change();
        match (a.kind, cs.after) {
            (ActivityKind::Create, Some(o)) => write_envelope(change.init_created(), &o.envelope),
            (_, Some(o)) => write_envelope(change.init_updated(), &o.envelope),
            (_, None) => {
                let mut b = change;
                write_uuid(b.reborrow().init_deleted(), Some(a.object.as_uuid()));
            }
        }
    }
    req.send().await
}
