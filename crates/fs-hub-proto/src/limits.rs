//! What fits into one hub message.
//!
//! A message above the reader's limit ends the whole connection with all its capabilities, so nobody
//! may ever send one. Three numbers, smallest first: a page is cut at `rpc::MAX_PAGE_BYTES` (32 MiB);
//! one object may have [`MAX_OBJECT_BYTES`] (16 MiB); one activity carries at most two images and the
//! envelope summary, so at most 48 MiB, which is below [`MAX_HUB_MESSAGE_BYTES`] (128 MiB) with room
//! for estimates that are a little off and for a reader that reads a value twice.
//!
//! Pattern: Specification — what is allowed, in one place, shared by hub and client; [`PageBudget`]
//! is a small Value object counting one answer.

use capnp::message::ReaderOptions;
use fs_interfaces::rpc::{
    MAX_INTAKE_BYTES, MAX_MESSAGE_BYTES, MAX_PAGE_BYTES, Problem, TOO_LARGE, envelope_weight,
};
use fs_objects::{Activity, Envelope};
use serde::Serialize;

/// Largest object the hub takes: one image, as JSON. The same limit every server has.
pub const MAX_OBJECT_BYTES: usize = MAX_INTAKE_BYTES;

/// Largest message on a hub connection, on both sides.
pub const MAX_HUB_MESSAGE_BYTES: usize = 2 * MAX_MESSAGE_BYTES;

/// Largest activity that can go into a message. Only an entry older than these limits gets near it:
/// the hub takes nothing that does.
pub const MAX_ACTIVITY_BYTES: usize = MAX_HUB_MESSAGE_BYTES - (1 << 20);

/// The fixed part of an activity on the wire: struct, pointers, ids, timestamps.
const ACTIVITY_FRAME_BYTES: usize = 256;

// The largest allowed activity (two images, the summary) fits a message.
const _: () = assert!(3 * MAX_OBJECT_BYTES + ACTIVITY_FRAME_BYTES < MAX_ACTIVITY_BYTES);

/// Reader options of a hub connection, on both sides.
pub fn reader_options() -> ReaderOptions {
    ReaderOptions {
        traversal_limit_in_words: Some(MAX_HUB_MESSAGE_BYTES / 8),
        ..ReaderOptions::default()
    }
}

struct Counter(usize);

impl std::io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Bytes `value` has as JSON. An activity's images count here, so an update counts its object twice.
pub fn json_len<T: Serialize>(value: &T) -> usize {
    let mut counter = Counter(0);
    match serde_json::to_writer(&mut counter, value) {
        Ok(()) => counter.0,
        // Cannot happen for a value that came from JSON; never call it small.
        Err(_) => usize::MAX,
    }
}

/// About what `a` weighs in a message with this summary. A `Null` payload stays empty on the wire.
pub fn activity_weight(a: &Activity, summary: Option<&Envelope>) -> usize {
    let payload = if a.payload.is_null() {
        0
    } else {
        json_len(&a.payload)
    };
    ACTIVITY_FRAME_BYTES
        .saturating_add(a.actor.len())
        .saturating_add(summary.map_or(0, envelope_weight))
        .saturating_add(payload)
}

/// The entry in `problems` for an object above [`MAX_OBJECT_BYTES`].
pub fn too_large(a: &Activity, bytes: usize) -> Problem {
    Problem::new(
        TOO_LARGE,
        "Too large",
        format!(
            "{} {}: object {} has {bytes} bytes, the hub takes at most {MAX_OBJECT_BYTES}; \
             refused, the log moved past it",
            a.kind.as_str(),
            a.id,
            a.object
        ),
        413,
    )
}

/// A failure that may pass: nothing is wrong with the entry, so no cursor may move past it.
pub fn is_transient(e: &fs_objects::Error) -> bool {
    matches!(e, fs_objects::Error::Backend(_) | fs_objects::Error::Key(_))
}

/// How many log entries to read at once. Each may hold two large images, and what does not fit is read
/// again next time; the store cannot filter by size. So this looks at the entries just read: the window
/// grows by one with each small entry up to a few, and a large one shrinks it to one again.
#[derive(Debug)]
pub struct ScanSize {
    window: usize,
}

impl Default for ScanSize {
    fn default() -> Self {
        Self { window: 1 }
    }
}

impl ScanSize {
    const MOST_ENTRIES: usize = 16;
    const LARGE_ENTRY_BYTES: usize = 256 << 10;

    /// Entries to read next.
    pub fn next(&self) -> usize {
        self.window
    }

    /// An entry of this payload was read.
    pub fn saw(&mut self, payload: &serde_json::Value) {
        self.window = if json_len(payload) >= Self::LARGE_ENTRY_BYTES {
            1
        } else {
            (self.window + 1).min(Self::MOST_ENTRIES)
        };
    }
}

/// How much of one answer or request is used.
#[derive(Debug, Default)]
pub struct PageBudget {
    used: usize,
}

impl PageBudget {
    /// Does an item of `weight` still go in? Always yes for the first, so paging always advances.
    pub fn admits(&mut self, weight: usize) -> bool {
        let next = self.used.saturating_add(weight);
        if self.used > 0 && next > MAX_PAGE_BYTES {
            return false;
        }
        self.used = next;
        true
    }

    /// Start over.
    pub fn reset(&mut self) {
        self.used = 0;
    }
}

#[cfg(test)]
mod tests;
