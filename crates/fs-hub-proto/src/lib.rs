//! `fs-hub-proto`: the Cap'n Proto contract of the hub (`schema/hub.capnp`) and conversions between the
//! wire types and `fs-store` types. Shared by the daemon and the client library.
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
pub use fs_interfaces::{common_capnp, hub_capnp};

mod content;
mod contract;
mod convert;
mod error;
mod types;

pub use content::same_content;
pub use contract::{CONTRACT_VERSION, default_socket_path};
pub use convert::*;
pub use error::{Error, Result};
pub use types::*;
