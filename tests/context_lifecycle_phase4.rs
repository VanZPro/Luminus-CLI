use luminus::context::{ContextBudget, TokenCount};

#[test]
fn accounts_prompt_and_assistant_deltas() {
    let mut budget = ContextBudget::new(100, 20);
    budget.account_user_prompt("one two three").unwrap();
    budget.account_assistant_delta("four five").unwrap();
    assert_eq!(budget.used_tokens(), TokenCount(5));
}

#[test]
fn response_reservation_is_updated_without_changing_usage() {
    let mut budget = ContextBudget::new(100, 20);
    budget.account_user_prompt("one").unwrap();
    budget.reserve_response(30);
    assert_eq!(budget.reserved_response_tokens(), TokenCount(30));
    assert_eq!(budget.used_tokens(), TokenCount(1));
}

#[test]
fn completion_and_cancellation_reset_usage() {
    let mut budget = ContextBudget::new(100, 20);
    budget.account_user_prompt("one two").unwrap();
    budget.complete_request();
    assert_eq!(budget.used_tokens(), TokenCount(0));
    assert_eq!(budget.reserved_response_tokens(), TokenCount(20));

    budget.account_assistant_delta("one two three").unwrap();
    budget.cancel_request();
    assert_eq!(budget.used_tokens(), TokenCount(0));
}
