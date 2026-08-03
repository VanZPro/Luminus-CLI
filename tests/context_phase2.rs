use luminus::context::{ContextBudget, TokenCount};

#[test]
fn accounts_against_response_reservation() {
    let mut budget = ContextBudget::new(1_000, 200);
    assert_eq!(budget.context_window(), TokenCount(1_000));
    assert_eq!(budget.reserved_response_tokens(), TokenCount(200));
    assert_eq!(budget.used_tokens(), TokenCount(0));
    assert_eq!(budget.available_tokens(), TokenCount(800));
    assert!(budget.can_fit(800));
    assert!(!budget.can_fit(801));
    budget.add_usage(300).unwrap();
    assert_eq!(budget.used_tokens(), TokenCount(300));
    assert!((budget.percentage() - 37.5).abs() < f64::EPSILON);
}

#[test]
fn rejects_overflow_without_mutating_usage() {
    let mut budget = ContextBudget::new(100, 20);
    assert!(budget.add_usage(81).is_err());
    assert_eq!(budget.used_tokens(), TokenCount(0));
}

#[test]
fn reset_and_compact_reclaim_context() {
    let mut budget = ContextBudget::new(100, 20);
    budget.add_usage(70).unwrap();
    budget.compact(30).unwrap();
    assert_eq!(budget.used_tokens(), TokenCount(40));
    budget.reset();
    assert_eq!(budget.used_tokens(), TokenCount(0));
}

#[test]
fn deterministic_approximation_counts_non_ascii_as_characters() {
    assert_eq!(ContextBudget::estimate_tokens("hello world"), TokenCount(2));
    assert_eq!(ContextBudget::estimate_tokens("éééé"), TokenCount(1));
}

#[test]
fn zero_effective_capacity_is_safe() {
    let budget = ContextBudget::new(100, 100);
    assert_eq!(budget.percentage(), 0.0);
    assert!(!budget.can_fit(1));
}

#[test]
fn compact_cannot_reclaim_more_than_used() {
    let mut budget = ContextBudget::new(100, 20);
    budget.add_usage(10).unwrap();
    assert!(budget.compact(11).is_err());
}
