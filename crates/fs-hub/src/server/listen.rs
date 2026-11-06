//! Accepting connections.
//!
//! Pattern: Adapter (transport) — Unix-socket streams become two-party capnp-rpc systems that
//! bootstrap the [`HubImpl`]; the peer must be the socket owner (`SO_PEERCRED`).

use std::path::PathBuf;
use std::sync::Arc;

use capnp::message::ReaderOptions;
use capnp_rpc::{RpcSystem, new_client, rpc_twoparty_capnp::Side, twoparty};
use fs_hub_proto::hub_capnp::hub;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::hub_service::HubImpl;
use crate::HubCore;

/// Serve the hub on an already bound listener until `shutdown` resolves. Must run on a `LocalSet`.
pub async fn serve(
    core: Arc<HubCore>,
    listener: UnixListener,
    socket: PathBuf,
    shutdown: impl Future<Output = ()>,
) {
    let owner =
        std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&socket).expect("socket metadata"));
    let bootstrap: hub::Client = new_client(HubImpl { core });
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                // Peer check: same user as the socket owner. (Real peer authentication is future work.)
                match stream.peer_cred() {
                    Ok(c) if c.uid() == owner => {}
                    _ => { tracing::warn!("rejected a connection from another user"); continue; }
                }
                spawn_connection(stream, bootstrap.clone());
            }
        }
    }
}

fn spawn_connection(stream: UnixStream, bootstrap: hub::Client) {
    let (r, w) = stream.into_split();
    let network = twoparty::VatNetwork::new(
        r.compat(),
        w.compat_write(),
        Side::Server,
        ReaderOptions::default(),
    );
    let rpc = RpcSystem::new(Box::new(network), Some(bootstrap.client));
    tokio::task::spawn_local(async move {
        if let Err(e) = rpc.await {
            tracing::debug!("connection ended: {e}");
        }
    });
}
