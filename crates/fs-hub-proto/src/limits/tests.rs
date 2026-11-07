//! What fits, and what counts as passing.
//!
//! Pattern: Specification checked by example.

use super::*;

#[test]
fn only_a_backend_or_key_failure_may_pass() {
    assert!(is_transient(&fs_objects::Error::Backend("disk".into())));
    assert!(is_transient(&fs_objects::Error::Key("locked".into())));
    assert!(!is_transient(&fs_objects::Error::Invalid("name".into())));
    assert!(!is_transient(&fs_objects::Error::Conflict("undo".into())));
}

#[test]
fn the_scan_window_slides() {
    let (small, large) = (
        serde_json::json!("x"),
        serde_json::json!("x".repeat(ScanSize::LARGE_ENTRY_BYTES)),
    );
    let mut scan = ScanSize::default();
    assert_eq!(scan.next(), 1);
    for _ in 0..100 {
        scan.saw(&small);
    }
    assert_eq!(scan.next(), ScanSize::MOST_ENTRIES);
    scan.saw(&large);
    assert_eq!(scan.next(), 1, "a large entry shrinks it at once");
    scan.saw(&small);
    assert_eq!(
        scan.next(),
        2,
        "and it does not stay small for the whole call"
    );
}

#[test]
fn a_budget_always_takes_the_first_and_cuts_after_that() {
    let mut page = PageBudget::default();
    assert!(
        page.admits(usize::MAX),
        "even too large: paging must advance"
    );
    assert!(!page.admits(1));
    page.reset();
    assert!(page.admits(MAX_PAGE_BYTES));
    assert!(!page.admits(1));
    page.reset();
    assert!(page.admits(MAX_PAGE_BYTES - 1));
    assert!(page.admits(1));
}
