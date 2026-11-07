//! Making a push fit.
//!
//! The hub reads a message of up to `MAX_HUB_MESSAGE_BYTES`; a larger one would end the connection.
//! So a push goes in pieces of at most one page, and what cannot go at all stays here as a problem,
//! the same one the hub would report, with the log moving past it. The hub only uses the `after`
//! image, so a large `before` image does not have to travel. Each image is looked at on its own: a
//! sensitive one only says that it is, because the hub never keeps it (ADR 0029) and drops any older
//! copy, and the other image of the same change, when the mark comes or goes, is the object itself.
//!
//! Pattern: Iterator-style batching — [`plan`] turns a batch of the log into pieces.

use chrono::{DateTime, Utc};
use fs_hub_proto::{
    MAX_OBJECT_BYTES, PageBudget, Problem, PushOutcome, activity_weight, images_of, json_len,
    sensitive_stub, summary_of, too_large,
};
use fs_objects::{Activity, Envelope, ObjectId};

/// What to send and what stays here.
#[derive(Debug, Default)]
pub(crate) struct PushPlan {
    /// One RPC call each, in log order.
    pub pieces: Vec<Vec<Activity>>,
    /// The entries that stay here, as if the hub had looked at them.
    pub refused: PushOutcome,
}

/// Cut `activities` into pieces that fit. Keeps the order; a piece holds at least one entry.
pub(crate) fn plan(mut activities: Vec<Activity>) -> PushPlan {
    // The hub's cursor moves over a whole piece: out of order, it would pass entries yet to come.
    activities.sort_by_key(|a| a.id);
    let mut out = PushPlan::default();
    let mut piece = Vec::new();
    let mut budget = PageBudget::default();
    for a in activities {
        let id = a.id;
        let a = match fit(a) {
            Ok(a) => a,
            Err(problem) => {
                out.refused.problems.push(*problem);
                out.refused.refused += 1;
                out.refused.last_accepted = out.refused.last_accepted.max(Some(id));
                continue;
            }
        };
        let weight = activity_weight(&a, summary_of(&a).as_ref());
        if !budget.admits(weight) {
            out.pieces.push(std::mem::take(&mut piece));
            budget.reset();
            budget.admits(weight);
        }
        piece.push(a);
    }
    if !piece.is_empty() {
        out.pieces.push(piece);
    }
    out
}

/// What can travel of `a`, or why nothing can.
fn fit(mut a: Activity) -> Result<Activity, Box<Problem>> {
    let Some((before, after)) = images_of(&a) else {
        // Log-only, or a bad entry the hub reports itself: only its size matters here.
        let bytes = json_len(&a.payload);
        if bytes > MAX_OBJECT_BYTES {
            return Err(Box::new(too_large(&a, bytes)));
        }
        return Ok(a);
    };
    let is_sensitive = |e: &Option<Envelope>| e.as_ref().is_some_and(|e| !e.sensitive.is_empty());
    let (before_sensitive, after_sensitive) = (is_sensitive(&before), is_sensitive(&after));
    let image_len = |a: &Activity, image| a.payload.get(image).map_or(0, json_len);
    if !after_sensitive && image_len(&a, "after") > MAX_OBJECT_BYTES {
        let bytes = image_len(&a, "after");
        return Err(Box::new(too_large(&a, bytes)));
    }
    if before_sensitive {
        a.payload["before"] = stub_image(a.object)?;
    } else if image_len(&a, "before") > MAX_OBJECT_BYTES {
        a.payload["before"] = serde_json::Value::Null;
    }
    if after_sensitive {
        a.payload["after"] = stub_image(a.object)?;
    }
    if before_sensitive || after_sensitive {
        a.at = DateTime::<Utc>::UNIX_EPOCH;
    }
    Ok(a)
}

/// All images of `a` as stubs, whatever they say: the object is marked now, so its past stays home too.
pub(crate) fn stub_images(mut a: Activity) -> Activity {
    let stub = sensitive_stub(a.object);
    for name in ["before", "after"] {
        if a.payload.get(name).is_some_and(|i| !i.is_null()) {
            // A stub is made of plain strings, and this cannot fail; never send the image if it does.
            a.payload[name] = serde_json::to_value(&stub).unwrap_or(serde_json::json!({
                "envelope": {"id": a.object, "sensitive": [fs_hub_proto::SENSITIVE_MARKER]}
            }));
        }
    }
    a.at = DateTime::<Utc>::UNIX_EPOCH;
    a
}

fn stub_image(id: ObjectId) -> Result<serde_json::Value, Box<Problem>> {
    serde_json::to_value(sensitive_stub(id)).map_err(|e| {
        Box::new(Problem::new(
            "about:blank",
            "Push problem",
            e.to_string(),
            422,
        ))
    })
}

#[cfg(test)]
mod tests;
