//! Plain Rust shapes of the contract structs.
//!
//! Pattern: Value objects (+ Builder-style setters on `Manifest`) — what callers handle instead of wire structs.

use fs_store::{ActivityId, Timestamp};
use serde::{Deserialize, Serialize};

use crate::Problem;

/// What a program tells the hub when it registers (ADR 0019).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Stable program name, `[a-z0-9._-]+`.
    pub program: String,
    /// Program version.
    pub version: String,
    /// Hub contract version the program was built for.
    pub contract_version: String,
    /// AS2 types the program wants to receive.
    pub reads: Vec<String>,
    /// AS2 types the program pushes.
    pub writes: Vec<String>,
    /// Content standards, e.g. `application/jscontact+json`.
    pub content_types: Vec<String>,
    /// Capability ids (ADR 0021), e.g. `contact.create`.
    pub capabilities: Vec<String>,
}

impl Manifest {
    /// Manifest for the current contract version.
    pub fn new(program: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            version: version.into(),
            contract_version: crate::CONTRACT_VERSION.into(),
            ..Self::default()
        }
    }
    /// Types the program wants to receive.
    #[must_use]
    pub fn read(mut self, types: &[&str]) -> Self {
        self.reads = types.iter().map(|s| (*s).to_string()).collect();
        self
    }
    /// Types the program pushes.
    #[must_use]
    pub fn write(mut self, types: &[&str]) -> Self {
        self.writes = types.iter().map(|s| (*s).to_string()).collect();
        self
    }
    /// Content standards the program handles.
    #[must_use]
    pub fn content(mut self, types: &[&str]) -> Self {
        self.content_types = types.iter().map(|s| (*s).to_string()).collect();
        self
    }
    /// Capability ids the program offers.
    #[must_use]
    pub fn capabilities(mut self, ids: &[&str]) -> Self {
        self.capabilities = ids.iter().map(|s| (*s).to_string()).collect();
        self
    }
}

/// What the hub knows about a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramInfo {
    /// Its manifest.
    pub manifest: Manifest,
    /// At least one live session.
    pub connected: bool,
    /// Last contact, unix milliseconds.
    pub last_seen_ms: i64,
    /// Last program activity the hub applied.
    pub last_accepted: Option<ActivityId>,
    /// Last hub activity the program acknowledged.
    pub pull_cursor: Option<ActivityId>,
}

/// A "same person" candidate pair found by the hub (ADR 0019 point 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    /// First object.
    pub a: fs_store::ObjectId,
    /// Second object.
    pub b: fs_store::ObjectId,
    /// Name of the first.
    pub name_a: String,
    /// Name of the second.
    pub name_b: String,
    /// Program that wrote the first.
    pub program_a: String,
    /// Program that wrote the second.
    pub program_b: String,
    /// Why they match, e.g. `email:ada@example.org`.
    pub reasons: Vec<String>,
}

/// Answer of `Session.push`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    /// Everything up to here is acknowledged: mark the log synced.
    pub last_accepted: Option<ActivityId>,
    /// Entries the hub applied.
    pub accepted: u32,
    /// Duplicates / already known.
    pub skipped: u32,
    /// Sensitive or undeclared types.
    pub withheld: u32,
    /// Problems reported for single entries.
    pub problems: Vec<Problem>,
}

impl PushOutcome {
    /// Add the outcome of the next piece of the same push.
    pub fn absorb(&mut self, next: Self) {
        self.last_accepted = self.last_accepted.max(next.last_accepted);
        self.accepted += next.accepted;
        self.skipped += next.skipped;
        self.withheld += next.withheld;
        self.problems.extend(next.problems);
    }
}

/// One activity as a program hands it over: what it did, and when, if it may say so.
#[derive(Clone, Debug)]
pub struct Pushed {
    /// The entry of the program's log.
    pub activity: fs_store::Activity,
    /// When the program did it; never set for a sensitive object (ADR 0029).
    pub original_at: Option<Timestamp>,
}

/// What an answer of `pull` says about one activity besides the activity itself.
#[derive(Clone, Debug, PartialEq)]
pub struct PullNote {
    /// Summary of the object (after image, else before image, else the object the hub holds).
    pub envelope: fs_store::Envelope,
    /// When the program did it; `Activity::at` is when the hub applied it.
    pub original_at: Option<Timestamp>,
}

/// Answer of `Session.pull`.
#[derive(Clone, Debug, Default)]
pub struct PullBatch {
    /// Activities of other programs. A `Null` payload on a state-changing kind means the content did
    /// not come: asked for with `pull_envelopes`, or too large for any message.
    pub activities: Vec<fs_store::Activity>,
    /// The note of each activity, by its id.
    pub notes: std::collections::BTreeMap<ActivityId, PullNote>,
    /// The hub scanned up to here; acknowledge with `ack`.
    pub last: Option<ActivityId>,
    /// More are waiting. An answer can hold fewer than asked for: it is also cut by size.
    pub more: bool,
}

/// A live notification (`Session.subscribe`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeNotice {
    /// An object was created.
    Created {
        /// Object id.
        id: fs_store::ObjectId,
        /// AS2 type.
        object_type: String,
        /// Display name.
        name: String,
    },
    /// An object was updated.
    Updated {
        /// Object id.
        id: fs_store::ObjectId,
        /// AS2 type.
        object_type: String,
        /// Display name.
        name: String,
    },
    /// An object was deleted.
    Deleted {
        /// Object id.
        id: fs_store::ObjectId,
    },
    /// The feed ended without being asked to: changes may have been missed. To go on, subscribe again
    /// and catch up with `pull`, in that order. Nothing follows this.
    Ended {
        /// Why it ended.
        problem: Problem,
    },
}
