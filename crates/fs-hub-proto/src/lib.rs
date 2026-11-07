//! `fs-hub-proto`: the Cap'n Proto contract of the hub (`schema/hub.capnp`) and conversions between the
//! wire types and `fs-objects` types. Shared by the daemon and the client library.
//!
//! Pattern: Adapter — `convert` translates between the generated wire code and plain Rust types.
//! The generated code comes from `fs-interfaces` (ADR 0040, 0045).

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// Noise for this crate: error/panic docs on every conversion, `#[must_use]` on getters, and the
// deliberate `as` casts for capnp list sizes and uuid halves (lengths are small, halves are masks).
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::doc_markdown
)]

// The schemas and the generated code live in `fs-interfaces` (ADR 0045); re-exported under the old paths.
pub use fs_interfaces::rpc::Problem;
pub use fs_interfaces::{common_capnp, hub_capnp};

mod content;
mod contract;
mod convert;
mod delivery;
mod error;
mod limits;
mod never_read;
mod redaction;
mod types;

pub use content::{same_as_written, same_content};
pub use contract::{CONTRACT_VERSION, default_socket_path};
pub use convert::*;
pub use delivery::{
    DELIVERED_INDEX_KEYS, SENSITIVE_MARKER, blank_for_delivery, blank_images, blank_object,
    keep_held_back, plain_delete_envelope, sensitive_stub, sensitive_stub_envelope,
};
pub use error::{Error, Result};
pub use limits::{
    MAX_ACTIVITY_BYTES, MAX_HUB_MESSAGE_BYTES, MAX_OBJECT_BYTES, PageBudget, ScanSize,
    activity_weight, is_transient, json_len, reader_options, too_large,
};
pub use never_read::{NEVER_READ, may_read};
pub use redaction::is_redacted;
pub use types::*;
