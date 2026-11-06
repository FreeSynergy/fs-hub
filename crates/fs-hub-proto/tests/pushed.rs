//! A push says when the program did it, unless the object is sensitive; `originalActor` stays empty.

use fs_hub_proto::hub_capnp::session::push_params;
use fs_hub_proto::{read_pushed_list, write_pushed_list};
use fs_store::{Activity, ActivityId, ActivityKind, ChangeSet, Envelope, ObjectId, StoredObject};

fn activity(sensitive: bool) -> Activity {
    let mut e = Envelope::new("Note", "n");
    if sensitive {
        e.sensitive = vec!["pd:Health".into()];
    }
    let object = StoredObject::new(e, "application/json", serde_json::json!({}));
    let cs = ChangeSet {
        before: None,
        after: Some(object),
    };
    Activity {
        id: ActivityId::new(),
        kind: ActivityKind::Create,
        object: ObjectId::new(),
        actor: "a person".into(),
        at: chrono::Utc::now(),
        target: None,
        payload: serde_json::to_value(cs).unwrap(),
        synced: false,
    }
}

#[test]
fn only_a_plain_object_says_when() {
    let items = [activity(false), activity(true)];
    let mut msg = capnp::message::Builder::new_default();
    let mut list = msg.init_root::<push_params::Builder>().init_activities(2);
    write_pushed_list(list.reborrow(), &items).unwrap();
    let reader = list.into_reader();
    assert!(reader.get(0).get_original_actor().unwrap().is_empty());
    let back = read_pushed_list(reader).unwrap();
    assert_eq!(
        back[0].original_at.map(|t| t.timestamp_millis()),
        Some(items[0].at.timestamp_millis())
    );
    assert_eq!(back[1].original_at, None);
}
