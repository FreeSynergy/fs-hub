//! The RPC calls themselves.
//!
//! Pattern: Adapter — one small function per remote method, converting through `fs-hub-proto`.

use fs_hub_proto::hub_capnp::{hub, session};
use fs_hub_proto::{
    Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome, read_program_info, read_proposal,
    read_pull_batch, read_push_outcome, write_activity_id, write_manifest, write_pushed_list,
};
use fs_objects::{Activity, ActivityId};

use crate::Result;

pub(super) async fn register(hub: &hub::Client, manifest: &Manifest) -> Result<session::Client> {
    let mut req = hub.register_request();
    write_manifest(req.get().init_manifest(), manifest);
    let resp = req.send().promise.await?;
    Ok(resp.get()?.get_session()?)
}

pub(super) async fn push(s: &session::Client, acts: &[Activity]) -> Result<PushOutcome> {
    let mut req = s.push_request();
    write_pushed_list(
        req.get()
            .init_activities(u32::try_from(acts.len()).expect("batch fits u32")),
        acts,
    )?;
    let resp = req.send().promise.await?;
    Ok(read_push_outcome(resp.get()?)?)
}

pub(super) async fn pull(
    s: &session::Client,
    since: Option<ActivityId>,
    limit: u32,
    envelope_only: bool,
) -> Result<PullBatch> {
    let mut req = s.pull_request();
    {
        let mut p = req.get();
        write_activity_id(p.reborrow().init_since(), since);
        p.set_limit(limit);
        p.set_envelope_only(envelope_only);
    }
    let resp = req.send().promise.await?;
    Ok(read_pull_batch(resp.get()?)?)
}

pub(super) async fn ack(s: &session::Client, id: ActivityId) -> Result<()> {
    let mut req = s.ack_request();
    write_activity_id(req.get().init_up_to(), Some(id));
    req.send().promise.await?;
    Ok(())
}

pub(super) async fn programs(hub: &hub::Client) -> Result<Vec<ProgramInfo>> {
    let resp = hub.programs_request().send().promise.await?;
    Ok(resp
        .get()?
        .get_programs()?
        .iter()
        .map(read_program_info)
        .collect::<std::result::Result<_, _>>()?)
}

pub(super) async fn proposals(hub: &hub::Client) -> Result<Vec<Proposal>> {
    let resp = hub.proposals_request().send().promise.await?;
    Ok(resp
        .get()?
        .get_proposals()?
        .iter()
        .map(read_proposal)
        .collect::<std::result::Result<_, _>>()?)
}
