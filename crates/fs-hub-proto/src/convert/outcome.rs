//! Push and pull results on the wire.
//!
//! Pattern: Adapter — `PushOutcome`/`PullBatch` <-> the `Session.push`/`pull` result structs.

use super::activity::read_activities;
use super::ids::{read_activity_id, write_activity_id};
use crate::hub_capnp as hub;
use crate::{PullBatch, PushOutcome, Result};

/// Wire -> push outcome.
pub fn read_push_outcome(r: hub::session::push_results::Reader<'_>) -> Result<PushOutcome> {
    let problems = r
        .get_problems()?
        .iter()
        .map(|p| Ok(p.get_detail()?.to_string()?))
        .collect::<Result<_>>()?;
    Ok(PushOutcome {
        last_accepted: read_activity_id(r.get_last_accepted()?)?,
        accepted: r.get_accepted(),
        skipped: r.get_skipped(),
        withheld: r.get_withheld(),
        problems,
    })
}

/// Push outcome -> wire.
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

/// Wire -> pull batch.
pub fn read_pull_batch(r: hub::session::pull_results::Reader<'_>) -> Result<PullBatch> {
    Ok(PullBatch {
        activities: read_activities(r.get_activities()?)?,
        last: read_activity_id(r.get_last()?)?,
        more: r.get_more(),
    })
}
