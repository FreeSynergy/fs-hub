//! Receiving live change notices.
//!
//! Pattern: Observer — the hub calls our `ChangeListener`; every call becomes a `ChangeNotice` on
//! a channel the program reads.

use std::rc::Rc;

use capnp_rpc::new_client;
use fs_hub_proto::common_capnp::{change, change_listener, subscription};
use fs_hub_proto::hub_capnp::session;
use fs_hub_proto::{ChangeNotice, read_uuid};
use fs_store::ObjectId;
use tokio::sync::mpsc;

use crate::Result;

struct Listener {
    tx: mpsc::UnboundedSender<ChangeNotice>,
}

impl change_listener::Server for Listener {
    // The generated trait method is async; nothing here awaits.
    #[allow(clippy::unused_async_trait_impl)]
    async fn change(
        self: Rc<Self>,
        params: change_listener::ChangeParams,
    ) -> std::result::Result<(), capnp::Error> {
        let notice = match params.get()?.get_change()?.which()? {
            change::Which::Created(e) => {
                let e = e?;
                ChangeNotice::Created {
                    id: object_id(e.get_id()?)?,
                    object_type: e.get_type()?.to_string()?,
                    name: e.get_name()?.to_string()?,
                }
            }
            change::Which::Updated(e) => {
                let e = e?;
                ChangeNotice::Updated {
                    id: object_id(e.get_id()?)?,
                    object_type: e.get_type()?.to_string()?,
                    name: e.get_name()?.to_string()?,
                }
            }
            change::Which::Deleted(u) => ChangeNotice::Deleted { id: object_id(u?)? },
        };
        let _ = self.tx.send(notice);
        Ok(())
    }
}

fn object_id(
    u: fs_hub_proto::common_capnp::uuid::Reader<'_>,
) -> std::result::Result<ObjectId, capnp::Error> {
    let u = read_uuid(u).ok_or_else(|| capnp::Error::failed("change without id".into()))?;
    ObjectId::from_uuid(u).map_err(|e| capnp::Error::failed(e.to_string()))
}

pub(super) async fn subscribe(
    s: &session::Client,
    tx: mpsc::UnboundedSender<ChangeNotice>,
) -> Result<subscription::Client> {
    let mut req = s.subscribe_request();
    req.get().set_listener(new_client(Listener { tx }));
    let resp = req.send().promise.await?;
    Ok(resp.get()?.get_subscription()?)
}
