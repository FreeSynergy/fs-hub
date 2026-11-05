//! Plain Rust shapes of the contract structs.

use fs_store::ActivityId;
use serde::{Deserialize, Serialize};

/// What a program tells the hub when it registers (ADR 0019).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Stable program name, `[a-z0-9._-]+`.
    pub program: String,
    pub version: String,
    pub contract_version: String,
    /// AS2 types the program wants to receive.
    pub reads: Vec<String>,
    /// AS2 types the program pushes.
    pub writes: Vec<String>,
    pub content_types: Vec<String>,
    /// Capability ids (ADR 0021), e.g. `contact.create`.
    pub capabilities: Vec<String>,
}

impl Manifest {
    pub fn new(program: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            version: version.into(),
            contract_version: crate::CONTRACT_VERSION.into(),
            ..Self::default()
        }
    }
    pub fn read(mut self, types: &[&str]) -> Self {
        self.reads = types.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn write(mut self, types: &[&str]) -> Self {
        self.writes = types.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn content(mut self, types: &[&str]) -> Self {
        self.content_types = types.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn capabilities(mut self, ids: &[&str]) -> Self {
        self.capabilities = ids.iter().map(|s| s.to_string()).collect();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramInfo {
    pub manifest: Manifest,
    pub connected: bool,
    pub last_seen_ms: i64,
    pub last_accepted: Option<ActivityId>,
    pub pull_cursor: Option<ActivityId>,
}

/// A "same person" candidate pair found by the hub (ADR 0019 point 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub a: fs_store::ObjectId,
    pub b: fs_store::ObjectId,
    pub name_a: String,
    pub name_b: String,
    pub program_a: String,
    pub program_b: String,
    pub reasons: Vec<String>,
}

/// Answer of `Session.push`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    /// Everything up to here is acknowledged: mark the log synced.
    pub last_accepted: Option<ActivityId>,
    pub accepted: u32,
    pub skipped: u32,
    pub withheld: u32,
    pub problems: Vec<String>,
}

/// Answer of `Session.pull`.
#[derive(Clone, Debug, Default)]
pub struct PullBatch {
    pub activities: Vec<fs_store::Activity>,
    /// The hub scanned up to here; acknowledge with `ack`.
    pub last: Option<ActivityId>,
    pub more: bool,
}

/// A live notification (`Session.subscribe`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeNotice {
    Created { id: fs_store::ObjectId, object_type: String, name: String },
    Updated { id: fs_store::ObjectId, object_type: String, name: String },
    Deleted { id: fs_store::ObjectId },
}
