//! Push and pull results on the wire.
//!
//! Pattern: Adapter — `PushOutcome`/`PullBatch` <-> the `Session.push`/`pull` result structs.

use super::activity::read_pulled_list;
use super::ids::{read_activity_id, write_activity_id};
use crate::common_capnp::problem;
use crate::hub_capnp as hub;
use crate::{Problem, PullBatch, PushOutcome, Result};

/// Wire -> problem.
pub fn read_problem(r: problem::Reader<'_>) -> Result<Problem> {
    Ok(Problem::new(
        r.get_type()?.to_string()?,
        r.get_title()?.to_string()?,
        r.get_detail()?.to_string()?,
        r.get_status(),
    ))
}

/// Wire -> push outcome.
pub fn read_push_outcome(r: hub::session::push_results::Reader<'_>) -> Result<PushOutcome> {
    let problems = r
        .get_problems()?
        .iter()
        .map(read_problem)
        .collect::<Result<_>>()?;
    Ok(PushOutcome {
        last_accepted: read_activity_id(r.get_last_accepted()?)?,
        accepted: r.get_accepted(),
        skipped: r.get_skipped(),
        withheld: r.get_withheld(),
        refused: 0,
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
        p.write(list.reborrow().get(i as u32));
    }
}

/// Wire -> pull batch.
pub fn read_pull_batch(r: hub::session::pull_results::Reader<'_>) -> Result<PullBatch> {
    let (activities, notes) = read_pulled_list(r.get_activities()?)?;
    Ok(PullBatch {
        activities,
        notes,
        last: read_activity_id(r.get_last()?)?,
        more: r.get_more(),
    })
}
