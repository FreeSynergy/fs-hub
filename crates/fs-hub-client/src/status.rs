//! Whether a hub is there.
//!
//! Pattern: State — `HubStatus` is either `Connected` (with a session) or `Absent`; callers use
//! the same methods in both states (`sync_once` is a no-op when absent), so a program works
//! the same with or without a hub.

use crate::{HubSession, Result, SyncReport};

/// Whether a hub is there.
pub enum HubStatus {
    /// Registered; use the session.
    Connected(HubSession),
    /// No hub (no socket, or nobody listens on it). The program works alone.
    Absent,
}

impl HubStatus {
    /// `true` in the `Connected` state.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    /// The session, when connected.
    pub fn session(&self) -> Option<&HubSession> {
        match self {
            Self::Connected(s) => Some(s),
            Self::Absent => None,
        }
    }

    /// `Some(report)` after a sync with the hub, `None` when there is no hub (nothing to do).
    pub async fn sync_once(&self, store: &dyn fs_store::Store) -> Result<Option<SyncReport>> {
        match self {
            Self::Connected(s) => Ok(Some(s.sync_once(store).await?)),
            Self::Absent => Ok(None),
        }
    }
}
