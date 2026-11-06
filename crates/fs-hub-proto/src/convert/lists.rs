//! Text lists.
//!
//! Pattern: Adapter — `capnp::text_list` to `Vec<String>` and back.

use crate::Result;

pub(super) fn text_list(r: capnp::text_list::Reader<'_>) -> Result<Vec<String>> {
    r.iter().map(|t| Ok(t?.to_string()?)).collect()
}

pub(super) fn set_text_list(mut b: capnp::text_list::Builder<'_>, items: &[String]) {
    for (i, s) in items.iter().enumerate() {
        b.set(u32::try_from(i).expect("list fits u32"), s.as_str());
    }
}
