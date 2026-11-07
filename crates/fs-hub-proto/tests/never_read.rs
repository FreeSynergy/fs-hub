//! Role assignments (`Relationship`) are not readable by any program.

use fs_hub_proto::{NEVER_READ, may_read};

#[test]
fn a_relationship_may_not_be_read() {
    assert!(!may_read("Relationship"));
    assert!(NEVER_READ.contains(&"Relationship"));
}

#[test]
fn the_types_programs_share_may_be_read() {
    for t in [
        "Profile",
        "Event",
        "Task",
        "Note",
        "relationship",
        "Relationships",
    ] {
        assert!(may_read(t), "{t}");
    }
}
