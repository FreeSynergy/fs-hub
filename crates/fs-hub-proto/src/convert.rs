//! Conversions between wire (Cap'n Proto) and `fs-store` / plain Rust types.

use chrono::{DateTime, TimeZone, Utc};
use fs_store::{Activity, ActivityId, ActivityKind, Envelope, ObjectId};
use uuid::Uuid;

use crate::common_capnp as common;
use crate::hub_capnp as hub;
use crate::{Error, Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome, Result};

// ---- ids and times ---------------------------------------------------------------------------

pub fn write_uuid(mut b: common::uuid::Builder<'_>, id: Option<Uuid>) {
    let v = id.map(|u| u.as_u128()).unwrap_or(0);
    b.set_hi((v >> 64) as u64);
    b.set_lo(v as u64);
}

/// All-zero = `None`.
pub fn read_uuid(r: common::uuid::Reader<'_>) -> Option<Uuid> {
    let v = (u128::from(r.get_hi()) << 64) | u128::from(r.get_lo());
    (v != 0).then(|| Uuid::from_u128(v))
}

pub fn read_activity_id(r: common::uuid::Reader<'_>) -> Result<Option<ActivityId>> {
    read_uuid(r).map(|u| ActivityId::from_uuid(u).map_err(Error::from)).transpose()
}

pub fn write_activity_id(b: common::uuid::Builder<'_>, id: Option<ActivityId>) {
    write_uuid(b, id.map(|i| i.as_uuid()));
}

fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

fn from_ms(v: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(v).single().unwrap_or_default()
}

fn text_list<'a>(r: capnp::text_list::Reader<'a>) -> Result<Vec<String>> {
    r.iter().map(|t| Ok(t?.to_string()?)).collect()
}

fn set_text_list(mut b: capnp::text_list::Builder<'_>, items: &[String]) {
    for (i, s) in items.iter().enumerate() {
        b.set(i as u32, s.as_str());
    }
}

// ---- envelope --------------------------------------------------------------------------------

/// Fill the typed routing summary of an object.
pub fn write_envelope(mut b: common::envelope::Builder<'_>, e: &Envelope) {
    write_uuid(b.reborrow().init_id(), Some(e.id.as_uuid()));
    b.set_iri(e.iri.as_deref().unwrap_or(""));
    b.set_type(&e.object_type);
    b.set_name(&e.name);
    b.set_attributed_to(e.attributed_to.as_deref().unwrap_or(""));
    set_text_list(b.reborrow().init_to(e.to.len() as u32), &e.to);
    set_text_list(b.reborrow().init_cc(e.cc.len() as u32), &e.cc);
    set_text_list(b.reborrow().init_bto(e.bto.len() as u32), &e.bto);
    set_text_list(b.reborrow().init_bcc(e.bcc.len() as u32), &e.bcc);
    set_text_list(b.reborrow().init_audience(e.audience.len() as u32), &e.audience);
    b.set_context(e.context.as_deref().unwrap_or(""));
    b.set_in_reply_to(e.in_reply_to.as_deref().unwrap_or(""));
    b.reborrow().init_published().set_unix_millis(ms(e.published));
    b.reborrow().init_updated().set_unix_millis(ms(e.updated));
    set_text_list(b.reborrow().init_sensitive(e.sensitive.len() as u32), &e.sensitive);
    b.set_describes(e.describes.as_deref().unwrap_or(""));
    if let Some(t) = e.start_time {
        b.reborrow().init_start_time().set_unix_millis(ms(t));
    }
    if let Some(t) = e.end_time {
        b.reborrow().init_end_time().set_unix_millis(ms(t));
    }
}

fn opt_text(r: capnp::text::Reader<'_>) -> Result<Option<String>> {
    let s = r.to_string()?;
    Ok((!s.is_empty()).then_some(s))
}

/// Read the typed envelope back (empty text = none, absent timestamp = none).
pub fn read_envelope(r: common::envelope::Reader<'_>) -> Result<Envelope> {
    let id = read_uuid(r.get_id()?).ok_or_else(|| Error::Invalid("envelope without id".into()))?;
    let mut e = Envelope::new(r.get_type()?.to_string()?, r.get_name()?.to_string()?);
    e.id = ObjectId::from_uuid(id)?;
    e.iri = opt_text(r.get_iri()?)?;
    e.attributed_to = opt_text(r.get_attributed_to()?)?;
    e.to = text_list(r.get_to()?)?;
    e.cc = text_list(r.get_cc()?)?;
    e.bto = text_list(r.get_bto()?)?;
    e.bcc = text_list(r.get_bcc()?)?;
    e.audience = text_list(r.get_audience()?)?;
    e.context = opt_text(r.get_context()?)?;
    e.in_reply_to = opt_text(r.get_in_reply_to()?)?;
    e.published = from_ms(r.get_published()?.get_unix_millis());
    e.updated = from_ms(r.get_updated()?.get_unix_millis());
    e.sensitive = text_list(r.get_sensitive()?)?;
    e.describes = opt_text(r.get_describes()?)?;
    e.start_time = if r.has_start_time() { Some(from_ms(r.get_start_time()?.get_unix_millis())) } else { None };
    e.end_time = if r.has_end_time() { Some(from_ms(r.get_end_time()?.get_unix_millis())) } else { None };
    Ok(e)
}

// ---- activity --------------------------------------------------------------------------------

/// The object envelope an activity is about (after image, else before image), if it carries one.
fn summary_of(a: &Activity) -> Option<Envelope> {
    if !a.kind.changes_state() {
        return None;
    }
    let cs = a.change_set().ok()?;
    cs.after.or(cs.before).map(|o| o.envelope)
}

pub fn write_activity(mut b: hub::activity::Builder<'_>, a: &Activity) -> Result<()> {
    write_activity_id(b.reborrow().init_id(), Some(a.id));
    b.set_kind(a.kind.as_str());
    write_uuid(b.reborrow().init_object(), Some(a.object.as_uuid()));
    b.set_actor(&a.actor);
    b.reborrow().init_at().set_unix_millis(ms(a.at));
    b.set_has_target(a.target.is_some());
    write_activity_id(b.reborrow().init_target(), a.target);
    match summary_of(a) {
        Some(env) => write_envelope(b.reborrow().init_envelope(), &env),
        None => write_uuid(b.reborrow().init_envelope().init_id(), Some(a.object.as_uuid())),
    }
    b.set_payload(&serde_json::to_vec(&a.payload)?);
    Ok(())
}

/// Wire activity -> store activity (`synced` is false: it is not in the receiver's log).
pub fn read_activity(r: hub::activity::Reader<'_>) -> Result<Activity> {
    let id = read_activity_id(r.get_id()?)?.ok_or_else(|| Error::Invalid("activity without id".into()))?;
    let object = read_uuid(r.get_object()?).ok_or_else(|| Error::Invalid("activity without object".into()))?;
    let kind: ActivityKind = r.get_kind()?.to_str()?.parse()?;
    let target = if r.get_has_target() { read_activity_id(r.get_target()?)? } else { None };
    Ok(Activity {
        id,
        kind,
        object: ObjectId::from_uuid(object)?,
        actor: r.get_actor()?.to_string()?,
        at: from_ms(r.get_at()?.get_unix_millis()),
        target,
        payload: serde_json::from_slice(r.get_payload()?)?,
        synced: false,
    })
}

pub fn write_activities(mut list: capnp::struct_list::Builder<'_, hub::activity::Owned>, items: &[Activity]) -> Result<()> {
    for (i, a) in items.iter().enumerate() {
        write_activity(list.reborrow().get(i as u32), a)?;
    }
    Ok(())
}

pub fn read_activities(list: capnp::struct_list::Reader<'_, hub::activity::Owned>) -> Result<Vec<Activity>> {
    list.iter().map(read_activity).collect()
}

// ---- manifest, program info, proposal --------------------------------------------------------

pub fn write_manifest(mut b: hub::manifest::Builder<'_>, m: &Manifest) {
    b.set_program(&m.program);
    b.set_version(&m.version);
    b.set_contract_version(&m.contract_version);
    set_text_list(b.reborrow().init_reads(m.reads.len() as u32), &m.reads);
    set_text_list(b.reborrow().init_writes(m.writes.len() as u32), &m.writes);
    set_text_list(b.reborrow().init_content_types(m.content_types.len() as u32), &m.content_types);
    let mut caps = b.reborrow().init_capabilities();
    caps.set_contract_version(&m.contract_version);
    set_text_list(caps.init_ids(m.capabilities.len() as u32), &m.capabilities);
}

pub fn read_manifest(r: hub::manifest::Reader<'_>) -> Result<Manifest> {
    let caps = r.get_capabilities()?;
    Ok(Manifest {
        program: r.get_program()?.to_string()?,
        version: r.get_version()?.to_string()?,
        contract_version: r.get_contract_version()?.to_string()?,
        reads: text_list(r.get_reads()?)?,
        writes: text_list(r.get_writes()?)?,
        content_types: text_list(r.get_content_types()?)?,
        capabilities: text_list(caps.get_ids()?)?,
    })
}

pub fn write_program_info(mut b: hub::program_info::Builder<'_>, p: &ProgramInfo) {
    write_manifest(b.reborrow().init_manifest(), &p.manifest);
    b.set_connected(p.connected);
    b.reborrow().init_last_seen().set_unix_millis(p.last_seen_ms);
    write_activity_id(b.reborrow().init_last_accepted(), p.last_accepted);
    write_activity_id(b.reborrow().init_pull_cursor(), p.pull_cursor);
}

pub fn read_program_info(r: hub::program_info::Reader<'_>) -> Result<ProgramInfo> {
    Ok(ProgramInfo {
        manifest: read_manifest(r.get_manifest()?)?,
        connected: r.get_connected(),
        last_seen_ms: r.get_last_seen()?.get_unix_millis(),
        last_accepted: read_activity_id(r.get_last_accepted()?)?,
        pull_cursor: read_activity_id(r.get_pull_cursor()?)?,
    })
}

pub fn write_proposal(mut b: hub::proposal::Builder<'_>, p: &Proposal) {
    write_uuid(b.reborrow().init_a(), Some(p.a.as_uuid()));
    write_uuid(b.reborrow().init_b(), Some(p.b.as_uuid()));
    b.set_name_a(&p.name_a);
    b.set_name_b(&p.name_b);
    b.set_program_a(&p.program_a);
    b.set_program_b(&p.program_b);
    set_text_list(b.init_reasons(p.reasons.len() as u32), &p.reasons);
}

pub fn read_proposal(r: hub::proposal::Reader<'_>) -> Result<Proposal> {
    let id = |u: common::uuid::Reader<'_>| -> Result<ObjectId> {
        Ok(ObjectId::from_uuid(read_uuid(u).ok_or_else(|| Error::Invalid("proposal without id".into()))?)?)
    };
    Ok(Proposal {
        a: id(r.get_a()?)?,
        b: id(r.get_b()?)?,
        name_a: r.get_name_a()?.to_string()?,
        name_b: r.get_name_b()?.to_string()?,
        program_a: r.get_program_a()?.to_string()?,
        program_b: r.get_program_b()?.to_string()?,
        reasons: text_list(r.get_reasons()?)?,
    })
}

// ---- push / pull results ---------------------------------------------------------------------

pub fn read_push_outcome(r: hub::session::push_results::Reader<'_>) -> Result<PushOutcome> {
    let problems = r.get_problems()?.iter().map(|p| Ok(p.get_detail()?.to_string()?)).collect::<Result<_>>()?;
    Ok(PushOutcome {
        last_accepted: read_activity_id(r.get_last_accepted()?)?,
        accepted: r.get_accepted(),
        skipped: r.get_skipped(),
        withheld: r.get_withheld(),
        problems,
    })
}

pub fn write_push_outcome(mut b: hub::session::push_results::Builder<'_>, o: &PushOutcome) {
    write_activity_id(b.reborrow().init_last_accepted(), o.last_accepted);
    b.set_accepted(o.accepted);
    b.set_skipped(o.skipped);
    b.set_withheld(o.withheld);
    let mut list = b.init_problems(o.problems.len() as u32);
    for (i, p) in o.problems.iter().enumerate() {
        let mut item = list.reborrow().get(i as u32);
        item.set_type("about:blank");
        item.set_title("push problem");
        item.set_detail(p);
        item.set_status(422);
    }
}

pub fn read_pull_batch(r: hub::session::pull_results::Reader<'_>) -> Result<PullBatch> {
    Ok(PullBatch {
        activities: read_activities(r.get_activities()?)?,
        last: read_activity_id(r.get_last()?)?,
        more: r.get_more(),
    })
}
