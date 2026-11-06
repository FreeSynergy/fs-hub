//! `Session` interface: push, pull, ack, subscribe.
//!
//! Pattern: Adapter — one session per registered program; the [`LiveGuard`] (RAII) counts it as
//! live until the capability is dropped.

use std::rc::Rc;
use std::sync::Arc;

use capnp_rpc::new_client;
use fs_hub_proto::hub_capnp::session;
use fs_hub_proto::{
    read_activities, read_activity_id, write_activities, write_activity_id, write_push_outcome,
};

use super::errors::rpc_err;
use super::subscription::{SubscriptionImpl, spawn_notifier};
use crate::{HubCore, LiveGuard};

const DEFAULT_PULL_LIMIT: usize = 200;

/// A program's session with the hub.
pub(super) struct SessionImpl {
    core: Arc<HubCore>,
    program: String,
    _live: LiveGuard,
}

impl SessionImpl {
    pub fn new(core: Arc<HubCore>, program: String, live: LiveGuard) -> Self {
        Self {
            core,
            program,
            _live: live,
        }
    }
}

impl session::Server for SessionImpl {
    async fn push(
        self: Rc<Self>,
        params: session::PushParams,
        mut results: session::PushResults,
    ) -> Result<(), capnp::Error> {
        let activities = read_activities(params.get()?.get_activities()?).map_err(rpc_err)?;
        let outcome = self
            .core
            .push(&self.program, activities)
            .await
            .map_err(rpc_err)?;
        write_push_outcome(results.get(), &outcome);
        Ok(())
    }

    async fn pull(
        self: Rc<Self>,
        params: session::PullParams,
        mut results: session::PullResults,
    ) -> Result<(), capnp::Error> {
        let (since, limit) = {
            let p = params.get()?;
            (
                read_activity_id(p.get_since()?).map_err(rpc_err)?,
                p.get_limit(),
            )
        };
        let limit = if limit == 0 {
            DEFAULT_PULL_LIMIT
        } else {
            limit as usize
        };
        let batch = self
            .core
            .pull(&self.program, since, limit)
            .await
            .map_err(rpc_err)?;
        let mut r = results.get();
        let count = u32::try_from(batch.activities.len()).map_err(rpc_err)?;
        write_activities(r.reborrow().init_activities(count), &batch.activities)
            .map_err(rpc_err)?;
        write_activity_id(r.reborrow().init_last(), batch.last);
        r.set_more(batch.more);
        Ok(())
    }

    async fn ack(
        self: Rc<Self>,
        params: session::AckParams,
        _: session::AckResults,
    ) -> Result<(), capnp::Error> {
        if let Some(up_to) = read_activity_id(params.get()?.get_up_to()?).map_err(rpc_err)? {
            self.core.ack(&self.program, up_to).await.map_err(rpc_err)?;
        }
        Ok(())
    }

    async fn subscribe(
        self: Rc<Self>,
        params: session::SubscribeParams,
        mut results: session::SubscribeResults,
    ) -> Result<(), capnp::Error> {
        let listener = params.get()?.get_listener()?;
        let reads = self
            .core
            .manifest(&self.program)
            .await
            .map_err(rpc_err)?
            .reads;
        let task = spawn_notifier(self.core.clone(), self.program.clone(), reads, listener);
        results
            .get()
            .set_subscription(new_client(SubscriptionImpl::new(task)));
        Ok(())
    }
}
