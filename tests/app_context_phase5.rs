use luminus::app::{App, Role};
use luminus::context::ContextBudget;
use luminus::event::ProviderEvent;

fn delta(request_id: &str, text: &str) -> ProviderEvent {
    ProviderEvent::Delta {
        request_id: request_id.into(),
        text: text.into(),
    }
}

#[test]
fn app_without_budget_works_normally() {
    let mut app = App::default();
    assert!(app.context_budget().is_none());
    
    app.start_request("r1".into(), "hello world".into());
    app.apply_provider_event(delta("r1", "response"));
    app.apply_provider_event(ProviderEvent::Completed {
        request_id: "r1".into(),
    });
    
    assert_eq!(app.messages.len(), 2);
    assert!(app.context_budget().is_none());
}

#[test]
fn set_context_budget_stores_budget() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    
    app.set_context_budget(budget);
    
    assert!(app.context_budget().is_some());
    assert_eq!(app.context_budget().unwrap().context_window().0, 1000);
}

#[test]
fn start_request_accounts_user_prompt() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    app.set_context_budget(budget);
    
    app.start_request("r1".into(), "hello world test".into());
    
    let budget = app.context_budget().unwrap();
    assert_eq!(budget.used_tokens().0, 3);
}

#[test]
fn delta_events_account_assistant_response() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    app.set_context_budget(budget);
    
    app.start_request("r1".into(), "hello".into());
    
    app.apply_provider_event(delta("r1", "Hi there"));
    app.apply_provider_event(delta("r1", " friend"));
    
    let budget = app.context_budget().unwrap();
    assert_eq!(budget.used_tokens().0, 4);
}

#[test]
fn completion_resets_budget() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    app.set_context_budget(budget);
    
    app.start_request("r1".into(), "hello world".into());
    app.apply_provider_event(delta("r1", "response text"));
    
    assert!(app.context_budget().unwrap().used_tokens().0 > 0);
    
    app.apply_provider_event(ProviderEvent::Completed {
        request_id: "r1".into(),
    });
    
    assert_eq!(app.context_budget().unwrap().used_tokens().0, 0);
}

#[test]
fn cancellation_resets_budget() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    app.set_context_budget(budget);
    
    app.start_request("r1".into(), "hello world".into());
    app.apply_provider_event(delta("r1", "partial response"));
    
    assert!(app.context_budget().unwrap().used_tokens().0 > 0);
    
    app.apply_provider_event(ProviderEvent::Cancelled {
        request_id: "r1".into(),
    });
    
    assert_eq!(app.context_budget().unwrap().used_tokens().0, 0);
}

#[test]
fn failure_resets_budget() {
    let mut app = App::default();
    let budget = ContextBudget::new(1000, 100);
    app.set_context_budget(budget);
    
    app.start_request("r1".into(), "hello world".into());
    app.apply_provider_event(delta("r1", "partial"));
    
    assert!(app.context_budget().unwrap().used_tokens().0 > 0);
    
    app.apply_provider_event(ProviderEvent::Failed {
        request_id: "r1".into(),
        error: "test error".into(),
    });
    
    assert_eq!(app.context_budget().unwrap().used_tokens().0, 0);
}
