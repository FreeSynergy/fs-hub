//! `fs-hub-client`: the library a program embeds to talk to the FreeSynergy hub (ADR 0019).
//!
//! * [`connect`] looks for the hub's socket. No hub: [`HubStatus::Absent`] and the program simply keeps
//!   working alone, writing its activity log; [`offer_install`] gives the text to ask the user.
//! * With a hub: register with a [`Manifest`], then [`HubSession::sync_once`] pushes the unsynced part of
//!   the log, pulls what other programs wrote (only declared types) into the program's store, and marks
//!   things synced.
//!
//! The RPC runs on a private thread (capnp-rpc is not `Send`), so the session is `Send + Sync`
//! and works from any tokio runtime.

mod error;
mod session;
mod sync;
mod worker;

use std::path::{Path, PathBuf};

pub use error::{Error, Result};
pub use fs_hub_proto::{ChangeNotice, Manifest, ProgramInfo, Proposal, PullBatch, PushOutcome};
pub use session::HubSession;
pub use sync::SyncReport;

/// Whether a hub is there.
pub enum HubStatus {
    /// Registered; use the session.
    Connected(HubSession),
    /// No hub (no socket, or nobody listens on it). The program works alone.
    Absent,
}

impl HubStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    /// `Some(report)` after a sync with the hub, `None` when there is no hub (nothing to do).
    pub async fn sync_once(&self, store: &dyn fs_store::Store) -> Result<Option<SyncReport>> {
        match self {
            Self::Connected(s) => Ok(Some(s.sync_once(store).await?)),
            Self::Absent => Ok(None),
        }
    }
}

/// Look for the hub at the default socket (`$XDG_RUNTIME_DIR/freesynergy/hub.sock`) and register.
pub async fn connect(manifest: Manifest) -> Result<HubStatus> {
    match fs_hub_proto::default_socket_path() {
        Some(path) => connect_at(&path, manifest).await,
        None => Ok(HubStatus::Absent),
    }
}

/// Like [`connect`] with an explicit socket path.
pub async fn connect_at(socket: &Path, manifest: Manifest) -> Result<HubStatus> {
    if !socket.exists() {
        return Ok(HubStatus::Absent);
    }
    match worker::start(PathBuf::from(socket), manifest).await {
        Ok(session) => Ok(HubStatus::Connected(session)),
        Err(Error::NoHub) => Ok(HubStatus::Absent), // stale socket file
        Err(e) => Err(e),
    }
}

/// What to say to the user when there is no hub (ADR 0019: the program asks whether to install it
/// and keeps running alone on "no"). Installing is not implemented yet; this only returns the message.
pub fn offer_install() -> String {
    "No FreeSynergy hub was found on this device. The hub lets your programs share their work \
     (for example a contact from Contacts showing up in Calendar). Install it now? \
     Automatic installation is not available yet; this program keeps working on its own and \
     hands over its history as soon as a hub is running."
        .to_string()
}
