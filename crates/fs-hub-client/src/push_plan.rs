//! Making a push fit.
//!
//! The hub reads a message of up to `MAX_HUB_MESSAGE_BYTES`; a larger one would end the connection.
//! So a push goes in pieces of at most one page, and what cannot go at all stays here as a problem,
//! the same one the hub would report, with the log moving past it. The hub only uses the `after`
//! image, so a large `before` image does not have to travel, and a sensitive object only has to say
//! that it is sensitive: the hub never keeps it (ADR 0029) and drops any older copy.
//!
//! Pattern: Iterator-style batching — [`plan`] turns a batch of the log into pieces.

use fs_hub_proto::{
    MAX_OBJECT_BYTES, PageBudget, Problem, PushOutcome, activity_weight, json_len, summary_of,
    too_large,
};
use fs_store::{Activity, ChangeSet, StoredObject};

/// What to send and what stays here.
#[derive(Debug, Default)]
pub(crate) struct PushPlan {
    /// One RPC call each, in log order.
    pub pieces: Vec<Vec<Activity>>,
    /// The entries that stay here, as if the hub had looked at them.
    pub refused: PushOutcome,
}

/// Cut `activities` into pieces that fit. Keeps the order; a piece holds at least one entry.
pub(crate) fn plan(activities: Vec<Activity>) -> PushPlan {
    let mut out = PushPlan::default();
    let mut piece = Vec::new();
    let mut budget = PageBudget::default();
    for a in activities {
        let id = a.id;
        let a = match fit(a) {
            Ok(a) => a,
            Err(problem) => {
                out.refused.problems.push(*problem);
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

/// The envelope alone: all the hub needs to know of a sensitive object.
fn stub(image: Option<StoredObject>) -> Option<StoredObject> {
    image.map(|o| StoredObject::new(o.envelope, o.content_type, serde_json::Value::Null))
}

fn is_sensitive(image: Option<&StoredObject>) -> bool {
    image.is_some_and(|o| !o.envelope.sensitive.is_empty())
}

/// What can travel of `a`, or why nothing can.
fn fit(mut a: Activity) -> Result<Activity, Box<Problem>> {
    let change = a.kind.changes_state().then(|| a.change_set().ok());
    let Some(Some(mut cs)) = change else {
        // Log-only, or a bad entry the hub reports itself: only its size matters here.
        let bytes = json_len(&a.payload);
        if bytes > MAX_OBJECT_BYTES {
            return Err(Box::new(too_large(&a, bytes)));
        }
        return Ok(a);
    };
    let is_large = |o: &StoredObject| json_len(o) > MAX_OBJECT_BYTES;
    if is_sensitive(cs.after.as_ref()) || is_sensitive(cs.before.as_ref()) {
        cs = ChangeSet {
            before: stub(cs.before),
            after: stub(cs.after),
        };
    } else if cs.after.as_ref().is_some_and(is_large) {
        let bytes = cs.after.as_ref().map_or(0, json_len);
        return Err(Box::new(too_large(&a, bytes)));
    } else if cs.before.as_ref().is_some_and(is_large) {
        cs.before = None;
    } else {
        return Ok(a);
    }
    match serde_json::to_value(&cs) {
        Ok(payload) => {
            a.payload = payload;
            Ok(a)
        }
        Err(e) => Err(Box::new(Problem::new(
            "about:blank",
            "Push problem",
            e.to_string(),
            422,
        ))),
    }
}

#[cfg(test)]
mod tests {
    use fs_hub_proto::MAX_HUB_MESSAGE_BYTES;
    use fs_store::{ActivityId, ActivityKind, Envelope, ObjectId};

    use super::*;

    fn image(bytes: usize, sensitive: bool) -> StoredObject {
        let mut e = Envelope::new("Note", "n");
        if sensitive {
            e.sensitive = vec!["pd:Health".into()];
        }
        let content = serde_json::json!({ "text": "x".repeat(bytes) });
        StoredObject::new(e, "application/json", content)
    }

    fn change(before: Option<StoredObject>, after: Option<StoredObject>) -> Activity {
        Activity {
            id: ActivityId::new(),
            kind: ActivityKind::Update,
            object: ObjectId::new(),
            actor: "person".into(),
            at: chrono::Utc::now(),
            target: None,
            payload: serde_json::to_value(ChangeSet { before, after }).unwrap(),
            synced: false,
        }
    }

    const LARGE: usize = MAX_OBJECT_BYTES + 1;

    #[test]
    fn pieces_keep_the_order_and_stay_below_a_page() {
        let log: Vec<_> = (0..80)
            .map(|_| change(None, Some(image(1 << 20, false))))
            .collect();
        let ids: Vec<_> = log.iter().map(|a| a.id).collect();
        let plan = plan(log);
        assert!(plan.pieces.len() > 1);
        let sent: Vec<_> = plan.pieces.iter().flatten().map(|a| a.id).collect();
        assert_eq!(sent, ids);
        for piece in &plan.pieces {
            let bytes: usize = piece.iter().map(|a| json_len(&a.payload)).sum();
            assert!(bytes < MAX_HUB_MESSAGE_BYTES / 2, "{bytes}");
        }
    }

    #[test]
    fn a_large_object_stays_here_and_the_log_moves_past_it() {
        let large = change(None, Some(image(LARGE, false)));
        let id = large.id;
        let plan = plan(vec![large]);
        assert_eq!(plan.pieces.len(), 0);
        assert_eq!(plan.refused.last_accepted, Some(id));
        assert_eq!(plan.refused.problems[0].status, 413);
        assert!(plan.refused.problems[0].detail.contains(&id.to_string()));
    }

    #[test]
    fn a_large_before_image_does_not_travel() {
        let plan = plan(vec![change(
            Some(image(LARGE, false)),
            Some(image(8, false)),
        )]);
        let cs = plan.pieces[0][0].change_set().unwrap();
        assert!(cs.before.is_none() && cs.after.is_some());
        assert_eq!(plan.refused.problems.len(), 0);
    }

    #[test]
    fn the_content_of_a_sensitive_object_never_travels_whatever_its_size() {
        for bytes in [8, LARGE] {
            let plan = plan(vec![change(
                Some(image(bytes, true)),
                Some(image(bytes, true)),
            )]);
            assert!(
                plan.refused.problems.is_empty(),
                "the hub has to hear of it"
            );
            let cs = plan.pieces[0][0].change_set().unwrap();
            let after = cs.after.unwrap();
            assert!(after.content.is_null() && cs.before.unwrap().content.is_null());
            assert_eq!(after.envelope.sensitive, ["pd:Health"]);
        }
    }
}
