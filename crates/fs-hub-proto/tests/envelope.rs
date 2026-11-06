//! The typed envelope carries `describes`, `startTime`, `endTime` (no longer only the JSON payload).

use chrono::{TimeZone, Utc};
use fs_hub_proto::common_capnp::envelope;
use fs_hub_proto::{read_envelope, write_envelope};
use fs_store::Envelope;

fn roundtrip(e: &Envelope) -> Envelope {
    let mut msg = capnp::message::Builder::new_default();
    write_envelope(msg.init_root::<envelope::Builder>(), e);
    read_envelope(msg.get_root_as_reader::<envelope::Reader>().unwrap()).unwrap()
}

#[test]
fn describes_and_times_travel_in_the_envelope() {
    let mut e = Envelope::new("Event", "Review");
    e.describes = Some("https://example.org/actors/ada".into());
    e.start_time = Some(Utc.timestamp_millis_opt(1_800_000_000_000).unwrap());
    e.end_time = Some(Utc.timestamp_millis_opt(1_800_003_600_000).unwrap());
    e.sensitive = vec!["pd:Health".into()];
    let back = roundtrip(&e);
    assert_eq!(back.describes, e.describes);
    assert_eq!(back.start_time, e.start_time);
    assert_eq!(back.end_time, e.end_time);
    assert_eq!(
        (back.id, back.object_type.as_str(), back.sensitive),
        (e.id, "Event", e.sensitive)
    );
}

#[test]
fn absent_optionals_stay_absent() {
    let back = roundtrip(&Envelope::new("Profile", "Ada"));
    assert!(back.describes.is_none() && back.start_time.is_none() && back.end_time.is_none());
    assert!(back.iri.is_none() && back.attributed_to.is_none());
}
