//! Ids and timestamps on the wire.
//!
//! Pattern: Adapter — `Uuid`/`ActivityId` and millisecond timestamps between Rust and the foreign Cap'n Proto structs (all-zero uuid = none).

use chrono::{DateTime, TimeZone, Utc};
use fs_store::ActivityId;
use uuid::Uuid;

use crate::common_capnp as common;
use crate::{Error, Result};

/// Write a uuid (`None` = all zero).
pub fn write_uuid(mut b: common::uuid::Builder<'_>, id: Option<Uuid>) {
    let v = id.map_or(0, |u| u.as_u128());
    b.set_hi((v >> 64) as u64);
    b.set_lo(v as u64);
}

/// All-zero = `None`.
pub fn read_uuid(r: common::uuid::Reader<'_>) -> Option<Uuid> {
    let v = (u128::from(r.get_hi()) << 64) | u128::from(r.get_lo());
    (v != 0).then(|| Uuid::from_u128(v))
}

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
