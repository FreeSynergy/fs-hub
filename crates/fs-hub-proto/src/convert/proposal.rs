//! Proposals on the wire.
//!
//! Pattern: Adapter — `Proposal` <-> `hub.capnp` `Proposal`.

use fs_objects::ObjectId;

use super::ids::{read_uuid, write_uuid};
use super::lists::{set_text_list, text_list};
use crate::common_capnp as common;
use crate::hub_capnp as hub;
use crate::{Error, Proposal, Result};

/// Proposal -> wire.
pub fn write_proposal(mut b: hub::proposal::Builder<'_>, p: &Proposal) {
    write_uuid(b.reborrow().init_a(), Some(p.a.as_uuid()));
    write_uuid(b.reborrow().init_b(), Some(p.b.as_uuid()));
    b.set_name_a(&p.name_a);
    b.set_name_b(&p.name_b);
    b.set_program_a(&p.program_a);
    b.set_program_b(&p.program_b);
    set_text_list(b.init_reasons(p.reasons.len() as u32), &p.reasons);
}

/// Wire -> proposal.
pub fn read_proposal(r: hub::proposal::Reader<'_>) -> Result<Proposal> {
    let id = |u: common::uuid::Reader<'_>| -> Result<ObjectId> {
        Ok(ObjectId::from_uuid(read_uuid(u).ok_or_else(|| {
            Error::Invalid("proposal without id".into())
        })?)?)
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
