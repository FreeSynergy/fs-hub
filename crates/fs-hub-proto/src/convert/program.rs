//! Program info on the wire.
//!
//! Pattern: Adapter — `ProgramInfo` <-> `hub.capnp` `ProgramInfo`.

use super::ids::{read_activity_id, write_activity_id};
use super::manifest::{read_manifest, write_manifest};
use crate::hub_capnp as hub;
use crate::{ProgramInfo, Result};

/// Program info -> wire.
pub fn write_program_info(mut b: hub::program_info::Builder<'_>, p: &ProgramInfo) {
    write_manifest(b.reborrow().init_manifest(), &p.manifest);
    b.set_connected(p.connected);
    b.reborrow()
        .init_last_seen()
        .set_unix_millis(p.last_seen_ms);
    write_activity_id(b.reborrow().init_last_accepted(), p.last_accepted);
    write_activity_id(b.reborrow().init_pull_cursor(), p.pull_cursor);
}

/// Wire -> program info.
pub fn read_program_info(r: hub::program_info::Reader<'_>) -> Result<ProgramInfo> {
    Ok(ProgramInfo {
        manifest: read_manifest(r.get_manifest()?)?,
        connected: r.get_connected(),
        last_seen_ms: r.get_last_seen()?.get_unix_millis(),
        last_accepted: read_activity_id(r.get_last_accepted()?)?,
        pull_cursor: read_activity_id(r.get_pull_cursor()?)?,
    })
}
