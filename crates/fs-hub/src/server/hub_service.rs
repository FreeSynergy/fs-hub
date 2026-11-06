//! `Hub` interface: register, programs, proposals.
//!
//! Pattern: Adapter — wire request in, [`HubCore`] call, wire response out.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use capnp_rpc::new_client;
use fs_hub_proto::hub_capnp::hub;
use fs_hub_proto::{CONTRACT_VERSION, read_manifest, write_program_info, write_proposal};

use super::errors::rpc_err;
use super::session_service::SessionImpl;
use crate::HubCore;

/// The bootstrap capability, one per connection: the contract has `proposals` here, without a session,
/// so this is where the connection's caller is known from its `register`.
pub(super) struct HubImpl {
    core: Arc<HubCore>,
    caller_reads: RefCell<Vec<String>>,
}

impl HubImpl {
    pub(super) fn new(core: Arc<HubCore>) -> Self {
        Self {
            core,
            caller_reads: RefCell::new(Vec::new()),
        }
    }
}

impl hub::Server for HubImpl {
    async fn register(
        self: Rc<Self>,
        params: hub::RegisterParams,
        mut results: hub::RegisterResults,
    ) -> Result<(), capnp::Error> {
        let manifest = read_manifest(params.get()?.get_manifest()?).map_err(rpc_err)?;
        let program = manifest.program.clone();
        let reads = manifest.reads.clone();
        let guard = self.core.register(manifest).await.map_err(rpc_err)?;
        *self.caller_reads.borrow_mut() = reads;
        let session = SessionImpl::new(self.core.clone(), program, guard);
        let mut r = results.get();
        r.set_session(new_client(session));
        r.set_hub_version(CONTRACT_VERSION);
        Ok(())
    }

    async fn programs(
        self: Rc<Self>,
        _: hub::ProgramsParams,
        mut results: hub::ProgramsResults,
    ) -> Result<(), capnp::Error> {
        let list = self.core.programs().await;
        let mut out = results
            .get()
            .init_programs(u32::try_from(list.len()).map_err(rpc_err)?);
        for (i, p) in list.iter().enumerate() {
            write_program_info(out.reborrow().get(i as u32), p);
        }
        Ok(())
    }

    async fn proposals(
        self: Rc<Self>,
        _: hub::ProposalsParams,
        mut results: hub::ProposalsResults,
    ) -> Result<(), capnp::Error> {
        let reads = self.caller_reads.borrow().clone();
        let list = self.core.proposals(&reads).await.map_err(rpc_err)?;
        let mut out = results
            .get()
            .init_proposals(u32::try_from(list.len()).map_err(rpc_err)?);
        for (i, p) in list.iter().enumerate() {
            write_proposal(out.reborrow().get(i as u32), p);
        }
        Ok(())
    }
}
