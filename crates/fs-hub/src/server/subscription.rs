//! Live change notices to a subscribed program.
//!
//! Pattern: Observer — a local task listens to the core's change feed, filters with the visibility
//! rule and hands notices to `rpc::forward`, which delivers them, ends a consumer that falls too far
//! behind or fails, and always says so through `ChangeListener.ended`. Dropping the `Subscription`
//! capability stops both, silently, as the consumer asked for it.
//!
//! When the feed lags (`RecvError::Lagged`) changes are gone. The contract has only `ended` to say
//! that, so the feed ends; the program subscribes again and catches up with `pull`.

use std::sync::Arc;

use fs_hub_proto::common_capnp::{change_listener, subscription};
use fs_hub_proto::{blank_for_delivery, images_of, is_redacted};
use fs_interfaces::rpc::{ChangeSender, Notice, change_channel, forward};
use fs_store::{Activity, ActivityId, ActivityKind, ObjectId};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinHandle;

use crate::HubCore;

/// What reaches the forwarder.
enum Event {
    /// Tell this, if it is still allowed by then.
    Tell {
        notice: Box<Notice>,
        object: ObjectId,
        entry: ActivityId,
    },
    /// Changes cannot be told anymore, for this reason.
    End(String),
}

/// Stops the feed task when the forwarding ends.
struct StopOnDrop(JoinHandle<()>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Start the feed for `listener`. The feed begins now, not when the first change comes.
pub(super) fn subscribe(
    core: Arc<HubCore>,
    program: String,
    reads: Vec<String>,
    listener: change_listener::Client,
) -> subscription::Client {
    let (sender, receiver) = change_channel();
    let changes = core.changes();
    let core_for_delivery = core.clone();
    let task = tokio::task::spawn_local(watch(core, program, reads, changes, sender));
    let at_delivery = core_for_delivery;
    forward(receiver, StopOnDrop(task), listener, move |event| {
        let core = at_delivery.clone();
        async move {
            match event {
                Event::Tell {
                    notice,
                    object,
                    entry,
                } => {
                    // A notice can wait in the queue, and the object can be marked meanwhile.
                    if matches!(*notice, Notice::Deleted(_)) {
                        return Ok(Some(*notice));
                    }
                    match core.withdrawn(object, entry).await {
                        Ok(false) => Ok(Some(*notice)),
                        Ok(true) => Ok(None),
                        Err(e) => Err(e.to_string()),
                    }
                }
                Event::End(why) => Err(why),
            }
        }
    })
}

/// Pass on what `program` may see. Returning drops the sender, which ends the feed as well.
async fn watch(
    core: Arc<HubCore>,
    program: String,
    reads: Vec<String>,
    mut changes: broadcast::Receiver<Activity>,
    sender: ChangeSender<Event>,
) {
    loop {
        match changes.recv().await {
            Ok(a) => {
                match core.visible_to(&program, &reads, &a).await {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        sender.send(Event::End(format!("{} {}: {e}", a.kind.as_str(), a.id)));
                        return;
                    }
                }
                match notice_of(&a) {
                    Ok(Some(notice)) => sender.send(Event::Tell {
                        notice: Box::new(notice),
                        object: a.object,
                        entry: a.id,
                    }),
                    Ok(None) => {}
                    Err(why) => {
                        sender.send(Event::End(why));
                        return;
                    }
                }
            }
            Err(RecvError::Lagged(missed)) => {
                sender.send(Event::End(format!(
                    "{missed} changes were missed; subscribe again and catch up with pull"
                )));
                return;
            }
            Err(RecvError::Closed) => return,
        }
    }
}

/// The notice of an activity, blanked like everything leaving the hub. A log-only activity changes no
/// object: nothing to tell.
pub(super) fn notice_of(a: &Activity) -> Result<Option<Notice>, String> {
    if !a.kind.changes_state() || is_redacted(a) {
        return Ok(None); // a redacted entry is no deletion: tell nothing
    }
    let (_, after) =
        images_of(a).ok_or_else(|| format!("{} {}: no images", a.kind.as_str(), a.id))?;
    let blanked = |mut e| {
        blank_for_delivery(&mut e);
        e
    };
    Ok(Some(match (a.kind, after) {
        (ActivityKind::Create, Some(e)) => Notice::Created(blanked(e)),
        (_, Some(e)) => Notice::Updated(blanked(e)),
        (_, None) => Notice::Deleted(a.object),
    }))
}
