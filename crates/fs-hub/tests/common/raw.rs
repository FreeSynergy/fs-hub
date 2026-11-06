//! A program talking to the hub on the wire, without `fs-hub-client`: to send what the client never
//! would and to see what really arrives. Runs inside a `LocalSet`.

use std::path::Path;

use fs_hub_client::{Manifest, PushOutcome};
use fs_hub_proto::hub_capnp::{hub, session};
use fs_hub_proto::{
    read_activity_id, read_push_outcome, write_activity_id, write_manifest, write_pushed_list,
};
use fs_interfaces::rpc::{Connection, connect};
use fs_store::{Activity, ActivityId};

pub struct Raw {
    pub session: session::Client,
    _connection: Connection<hub::Client>,
}

/// One activity as the wire has it.
#[derive(Debug)]
pub struct Seen {
    pub id: ActivityId,
    pub kind: String,
    pub payload_len: usize,
    pub payload: String,
    /// The typed envelope's audience fields, as they came.
    pub audience: Vec<String>,
    pub original_at_ms: Option<i64>,
    pub original_actor: String,
    pub actor: String,
    pub at_ms: i64,
    pub object_type: String,
    pub name: String,
}

pub struct Answer {
    pub activities: Vec<Seen>,
    pub last: Option<ActivityId>,
    pub more: bool,
}

pub async fn register(socket: &Path, manifest: &Manifest) -> Raw {
    let connection: Connection<hub::Client> = connect(socket).await.unwrap();
    let mut request = connection.client.register_request();
    write_manifest(request.get().init_manifest(), manifest);
    let response = request.send().promise.await.unwrap();
    Raw {
        session: response.get().unwrap().get_session().unwrap(),
        _connection: connection,
    }
}

impl Raw {
    /// Push `activities` as they are, each claiming `original_actor`.
    pub async fn push(&self, activities: &[Activity], original_actor: &str) -> PushOutcome {
        self.push_claiming(activities, original_actor, None).await
    }

    /// Like [`Raw::push`], and each also claims `original_at_ms`, whatever it is about.
    pub async fn push_claiming(
        &self,
        activities: &[Activity],
        original_actor: &str,
        original_at_ms: Option<i64>,
    ) -> PushOutcome {
        let mut request = self.session.push_request();
        {
            let count = u32::try_from(activities.len()).unwrap();
            let mut list = request.get().init_activities(count);
            write_pushed_list(list.reborrow(), activities).unwrap();
            for i in 0..count {
                let mut item = list.reborrow().get(i);
                item.set_original_actor(original_actor);
                if let Some(ms) = original_at_ms {
                    item.init_original_at().set_unix_millis(ms);
                }
            }
        }
        let response = request.send().promise.await.unwrap();
        read_push_outcome(response.get().unwrap()).unwrap()
    }

    pub async fn ack(&self, up_to: ActivityId) -> Result<(), capnp::Error> {
        let mut request = self.session.ack_request();
        write_activity_id(request.get().init_up_to(), Some(up_to));
        request.send().promise.await.map(|_| ())
    }

    pub async fn pull(&self, since: Option<ActivityId>, limit: u32, envelope_only: bool) -> Answer {
        let mut request = self.session.pull_request();
        {
            let mut p = request.get();
            write_activity_id(p.reborrow().init_since(), since);
            p.set_limit(limit);
            p.set_envelope_only(envelope_only);
        }
        let response = request.send().promise.await.unwrap();
        let r = response.get().unwrap();
        let activities = r.get_activities().unwrap().iter().map(seen).collect();
        Answer {
            activities,
            last: read_activity_id(r.get_last().unwrap()).unwrap(),
            more: r.get_more(),
        }
    }
}

fn text(t: capnp::text::Reader<'_>) -> String {
    t.to_string().unwrap()
}

fn seen(a: fs_hub_proto::hub_capnp::activity::Reader<'_>) -> Seen {
    let envelope = a.get_envelope().unwrap();
    Seen {
        id: read_activity_id(a.get_id().unwrap()).unwrap().unwrap(),
        kind: text(a.get_kind().unwrap()),
        payload_len: a.get_payload().unwrap().len(),
        payload: String::from_utf8_lossy(a.get_payload().unwrap()).into_owned(),
        audience: audience_of(envelope),
        original_at_ms: a
            .has_original_at()
            .then(|| a.get_original_at().unwrap().get_unix_millis()),
        original_actor: text(a.get_original_actor().unwrap()),
        actor: text(a.get_actor().unwrap()),
        at_ms: a.get_at().unwrap().get_unix_millis(),
        object_type: text(envelope.get_type().unwrap()),
        name: text(envelope.get_name().unwrap()),
    }
}

/// Every field that may name a person, as one list; empty if none is set.
pub fn audience_of(e: fs_hub_proto::common_capnp::envelope::Reader<'_>) -> Vec<String> {
    let mut out = vec![text(e.get_attributed_to().unwrap())];
    for list in [
        e.get_to().unwrap(),
        e.get_cc().unwrap(),
        e.get_bto().unwrap(),
        e.get_bcc().unwrap(),
        e.get_audience().unwrap(),
    ] {
        out.extend(list.iter().map(|t| text(t.unwrap())));
    }
    out.retain(|s| !s.is_empty());
    out
}
