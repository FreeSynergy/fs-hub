//! Ids and timestamps on the wire.
//!
//! Pattern: Adapter — `Uuid`/`ActivityId` and millisecond timestamps between Rust and the foreign Cap'n Proto structs (all-zero uuid = none).

use chrono::{DateTime, TimeZone, Utc};
use fs_store::ActivityId;

pub use fs_interfaces::rpc::{read_uuid, write_uuid};

use crate::common_capnp as common;
use crate::{Error, Result};

/// Read an activity id (`None` = all zero).
pub fn read_activity_id(r: common::uuid::Reader<'_>) -> Result<Option<ActivityId>> {
    read_uuid(r)
        .map(|u| ActivityId::from_uuid(u).map_err(Error::from))
        .transpose()
}

/// Write an activity id (`None` = all zero).
pub fn write_activity_id(b: common::uuid::Builder<'_>, id: Option<ActivityId>) {
    write_uuid(b, id.map(|i| i.as_uuid()));
}

/// `DateTime` -> unix milliseconds.
pub(super) fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}

/// Unix milliseconds -> `DateTime` (out of range = epoch).
pub(super) fn from_ms(v: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(v).single().unwrap_or_default()
}
