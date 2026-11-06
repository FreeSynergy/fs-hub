//! The private RPC thread.
//!
//! Pattern: Adapter — owns the capnp-rpc connection (not `Send`) on its own thread and turns
//! [`Cmd`](crate::command::Cmd) values into RPC calls.

mod calls;
mod listener;
mod run;
mod thread;

pub(crate) use thread::start;
