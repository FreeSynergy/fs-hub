//! The envelopes inside an activity's before/after images.
//!
//! `Activity::change_set` copies the whole payload, and it may hold two large images. Reading only the
//! envelopes through the `&Value` deserializer copies no content.
//!
//! Pattern: Adapter — the JSON of a change set -> what routing needs of it.

use fs_store::{Activity, Envelope};
use serde::Deserialize;

#[derive(Deserialize)]
struct Image {
    envelope: Envelope,
}

#[derive(Deserialize)]
struct Images {
    before: Option<Image>,
    after: Option<Image>,
}

/// The envelopes of a state-changing activity: before image, after image.
pub fn images_of(a: &Activity) -> Option<(Option<Envelope>, Option<Envelope>)> {
    if !a.kind.changes_state() {
        return None;
    }
    let images = Images::deserialize(&a.payload).ok()?;
    Some((
        images.before.map(|i| i.envelope),
        images.after.map(|i| i.envelope),
    ))
}

/// The object envelope an activity is about (after image, else before image), if it carries one.
pub fn summary_of(a: &Activity) -> Option<Envelope> {
    let (before, after) = images_of(a)?;
    after.or(before)
}
