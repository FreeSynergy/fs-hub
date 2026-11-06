//! Program manifests on the wire.
//!
//! Pattern: Adapter — `Manifest` <-> `hub.capnp` `Manifest` (with the capability list of `common.capnp`).

use super::lists::{set_text_list, text_list};
use crate::hub_capnp as hub;
use crate::{Manifest, Result};

/// Manifest -> wire.
pub fn write_manifest(mut b: hub::manifest::Builder<'_>, m: &Manifest) {
    b.set_program(&m.program);
    b.set_version(&m.version);
    b.set_contract_version(&m.contract_version);
    set_text_list(b.reborrow().init_reads(m.reads.len() as u32), &m.reads);
    set_text_list(b.reborrow().init_writes(m.writes.len() as u32), &m.writes);
    set_text_list(
        b.reborrow()
            .init_content_types(m.content_types.len() as u32),
        &m.content_types,
    );
    let mut caps = b.reborrow().init_capabilities();
    caps.set_contract_version(&m.contract_version);
    set_text_list(caps.init_ids(m.capabilities.len() as u32), &m.capabilities);
}

/// Wire -> manifest.
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
