//! Types no program may read through the hub.
//!
//! Pattern: Specification — a pure predicate over an object type. A role assignment is an AS2
//! `Relationship` (`roles.md`); that someone is another's customer is not everyone's business
//! (ADR 0029), and the hub decides by type. So no manifest may declare `reads` of these, and nothing of
//! such a type is delivered whatever an old manifest says. The hub itself reads them (same-person links).

/// The types no program gets to read. The only list.
pub const NEVER_READ: &[&str] = &["Relationship"];

/// May a program declare `reads` of this type?
pub fn may_read(object_type: &str) -> bool {
    !NEVER_READ.contains(&object_type)
}
