//! Which programs have a live session.
//!
//! Pattern: RAII / Guard — `enter` counts a session, dropping the [`LiveGuard`] uncounts it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Live-session counts per program.
#[derive(Default)]
pub struct Presence {
    live: Mutex<HashMap<String, usize>>,
}

/// Decrements the live-session count of a program when the session ends.
pub struct LiveGuard {
    presence: Arc<Presence>,
    program: String,
}

impl Presence {
    /// Count one more session of `program`.
    pub fn enter(self: &Arc<Self>, program: &str) -> LiveGuard {
        *self
            .live
            .lock()
            .unwrap()
            .entry(program.to_owned())
            .or_default() += 1;
        LiveGuard {
            presence: self.clone(),
            program: program.to_owned(),
        }
    }

    /// Does `program` have at least one live session?
    pub fn is_live(&self, program: &str) -> bool {
        self.live.lock().unwrap().get(program).copied().unwrap_or(0) > 0
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut live = self.presence.live.lock().unwrap();
        if let Some(n) = live.get_mut(&self.program) {
            *n = n.saturating_sub(1);
        }
    }
}
