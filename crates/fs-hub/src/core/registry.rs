//! The set of programs the hub knows.
//!
//! Pattern: Strategy + Registry — `ProgramRegistry` is the interface the push/pull logic depends
//! on; `StoredRegistry` keeps states in memory (one async mutex per program) and persists them
//! through a `CursorRepository`.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fs_hub_proto::{CONTRACT_VERSION, Manifest, ProgramInfo};
use fs_objects::ObjectId;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use super::cursors::CursorRepository;
use super::presence::{LiveGuard, Presence};
use super::state::{ProgramState, now_ms};
use crate::{Error, Result};

/// Exclusive access to one program's state; `save` persists it.
pub struct ProgramLock {
    guard: OwnedMutexGuard<ProgramState>,
    cursors: Arc<dyn CursorRepository>,
}

impl ProgramLock {
    /// Persist the (modified) state.
    pub async fn save(&self) -> Result<()> {
        self.cursors.save(&self.guard).await
    }
}

impl Deref for ProgramLock {
    type Target = ProgramState;
    fn deref(&self) -> &ProgramState {
        &self.guard
    }
}

impl DerefMut for ProgramLock {
    fn deref_mut(&mut self) -> &mut ProgramState {
        &mut self.guard
    }
}

/// Registration and lookup of programs.
#[async_trait]
pub trait ProgramRegistry: Send + Sync {
    /// A program announces itself; the guard counts the session as live.
    async fn register(&self, manifest: Manifest) -> Result<LiveGuard>;
    /// Manifest of a registered program.
    async fn manifest(&self, program: &str) -> Result<Manifest>;
    /// All programs the hub knows (connected or not), sorted by name.
    async fn programs(&self) -> Vec<ProgramInfo>;
    /// Exclusive access to a program's state (serialises its pushes).
    async fn lock(&self, program: &str) -> Result<ProgramLock>;
    /// Persist every state, at shutdown: a push does not save each time.
    async fn flush(&self) -> Result<()>;
}

type Shared = Arc<AsyncMutex<ProgramState>>;

/// [`ProgramRegistry`] persisting through a [`CursorRepository`].
pub struct StoredRegistry {
    cursors: Arc<dyn CursorRepository>,
    programs: Mutex<HashMap<String, Shared>>,
    presence: Arc<Presence>,
}

fn valid_program_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

impl StoredRegistry {
    /// Load the stored states.
    pub async fn open(cursors: Arc<dyn CursorRepository>) -> Result<Self> {
        let mut map = HashMap::new();
        for st in cursors.load_all().await? {
            map.insert(st.manifest.program.clone(), Arc::new(AsyncMutex::new(st)));
        }
        Ok(Self {
            cursors,
            programs: Mutex::new(map),
            presence: Arc::default(),
        })
    }

    fn shared(&self, program: &str) -> Result<Shared> {
        self.programs
            .lock()
            .unwrap()
            .get(program)
            .cloned()
            .ok_or_else(|| Error::Invalid(format!("unknown program {program}")))
    }
}

#[async_trait]
impl ProgramRegistry for StoredRegistry {
    async fn register(&self, manifest: Manifest) -> Result<LiveGuard> {
        if !valid_program_name(&manifest.program) {
            return Err(Error::Invalid(format!(
                "invalid program name {:?} (use [a-z0-9._-]+)",
                manifest.program
            )));
        }
        if manifest.contract_version != CONTRACT_VERSION {
            return Err(Error::Invalid(format!(
                "contract version {:?} not supported (hub speaks {CONTRACT_VERSION})",
                manifest.contract_version
            )));
        }
        let name = manifest.program.clone();
        let existing = self.programs.lock().unwrap().get(&name).cloned();
        if let Some(shared) = existing {
            let mut st = shared.lock().await;
            st.manifest = manifest;
            st.last_seen_ms = now_ms();
            self.cursors.save(&st).await?;
        } else {
            let st = ProgramState {
                object_id: ObjectId::new(),
                manifest,
                last_accepted: None,
                pull_cursor: None,
                last_seen_ms: now_ms(),
                held: Vec::new(),
                settled_ms: 0,
            };
            self.cursors.save(&st).await?;
            self.programs
                .lock()
                .unwrap()
                .insert(name.clone(), Arc::new(AsyncMutex::new(st)));
        }
        Ok(self.presence.enter(&name))
    }

    async fn manifest(&self, program: &str) -> Result<Manifest> {
        Ok(self.shared(program)?.lock().await.manifest.clone())
    }

    async fn programs(&self) -> Vec<ProgramInfo> {
        let all: Vec<(String, Shared)> = self
            .programs
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut out = vec![];
        for (name, shared) in all {
            let st = shared.lock().await;
            out.push(ProgramInfo {
                manifest: st.manifest.clone(),
                connected: self.presence.is_live(&name),
                last_seen_ms: st.last_seen_ms,
                last_accepted: st.last_accepted,
                pull_cursor: st.pull_cursor,
            });
        }
        out.sort_by(|a, b| a.manifest.program.cmp(&b.manifest.program));
        out
    }

    async fn lock(&self, program: &str) -> Result<ProgramLock> {
        let guard = self.shared(program)?.lock_owned().await;
        Ok(ProgramLock {
            guard,
            cursors: self.cursors.clone(),
        })
    }

    async fn flush(&self) -> Result<()> {
        let all: Vec<Shared> = self.programs.lock().unwrap().values().cloned().collect();
        for shared in all {
            self.cursors.save(&*shared.lock().await).await?;
        }
        Ok(())
    }
}
