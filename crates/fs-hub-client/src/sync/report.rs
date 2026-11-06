//! The result of one sync.
//!
//! Pattern: Value object — counters and problems, nothing else.

/// What one sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Log entries sent.
    pub pushed: usize,
    /// ... of which the hub applied.
    pub accepted: u32,
    /// ... already known to the hub.
    pub skipped: u32,
    /// ... not collected (sensitive, ADR 0029; or not a declared `writes` type).
    pub withheld: u32,
    /// Problems the hub reported for single entries.
    pub problems: Vec<String>,
    /// Activities received from other programs.
    pub pulled: usize,
    /// ... changing the local store.
    pub applied: usize,
    /// ... that could not be applied (reason).
    pub failed: Vec<String>,
}
