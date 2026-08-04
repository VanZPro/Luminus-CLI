use crate::context::ContextBudget;
use crate::event::ProviderEvent;
use crate::tool_activity::ToolActivity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestPhase {
    Streaming,
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRequest {
    request_id: String,
    phase: RequestPhase,
    message_index: Option<usize>,
}

/// Application UI mode: normal chat or a modal overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiMode {
    #[default]
    Normal,
    ModelSelector,
}

/// Pure application state: a reducer over user input and provider events.
#[derive(Debug, Default)]
pub struct App {
    pub messages: Vec<Message>,
    pub tool_activities: Vec<ToolActivity>,
    request: Option<ActiveRequest>,
    context_budget: Option<ContextBudget>,
    ui_mode: UiMode,
    pub model_selector_index: usize,
    pub model_selector_items: Vec<String>,
}

impl App {
    /// Current UI mode.
    pub fn ui_mode(&self) -> UiMode {
        self.ui_mode
    }

    /// Show the model selector overlay with the given display items.
    pub fn show_model_selector(&mut self, items: Vec<String>) {
        self.ui_mode = UiMode::ModelSelector;
        self.model_selector_index = 0;
        self.model_selector_items = items;
    }

    /// Hide the model selector overlay.
    pub fn hide_model_selector(&mut self) {
        self.ui_mode = UiMode::Normal;
        self.model_selector_items.clear();
    }

    /// Move the model selector cursor up.
    pub fn model_selector_prev(&mut self) {
        self.model_selector_index = self.model_selector_index.saturating_sub(1);
    }

    /// Move the model selector cursor down, clamped to the item count.
    pub fn model_selector_next(&mut self) {
        let max = self.model_selector_items.len().saturating_sub(1);
        if self.model_selector_index < max {
            self.model_selector_index += 1;
        }
    }

    /// Sets the context budget used for token accounting.
    pub fn set_context_budget(&mut self, budget: ContextBudget) {
        self.context_budget = Some(budget);
    }

    /// Current context budget, when configured.
    pub fn context_budget(&self) -> Option<&ContextBudget> {
        self.context_budget.as_ref()
    }

    /// Records the user's prompt and begins tracking the request lifecycle.
    pub fn start_request(&mut self, request_id: String, prompt: String) {
        if let Some(budget) = &mut self.context_budget {
            let _ = budget.account_user_prompt(&prompt);
        }
        self.messages.push(Message {
            role: Role::User,
            content: prompt,
        });
        self.request = Some(ActiveRequest {
            request_id,
            phase: RequestPhase::Streaming,
            message_index: None,
        });
        self.tool_activities.clear();
    }

    /// Applies a provider event; events for unknown or terminated requests are ignored.
    pub fn apply_provider_event(&mut self, event: ProviderEvent) {
        let Some(request) = self.request.as_mut() else {
            return;
        };
        if request.request_id != event.request_id() || request.phase == RequestPhase::Terminated {
            return;
        }

        match event {
            ProviderEvent::Started { .. } => {}
            ProviderEvent::ToolStarted { activity, .. }
            | ProviderEvent::ToolProgress { activity, .. }
            | ProviderEvent::ToolCompleted { activity, .. }
            | ProviderEvent::ToolFailed { activity, .. } => self.tool_activities.push(activity),
            ProviderEvent::Delta { text, .. } => {
                if let Some(budget) = &mut self.context_budget {
                    let _ = budget.account_assistant_delta(&text);
                }
                match request.message_index {
                    Some(index) => self.messages[index].content.push_str(&text),
                    None => {
                        request.message_index = Some(self.messages.len());
                        self.messages.push(Message {
                            role: Role::Assistant,
                            content: text,
                        });
                    }
                }
            }
            ProviderEvent::Completed { .. } => {
                request.phase = RequestPhase::Terminated;
                if let Some(budget) = &mut self.context_budget {
                    budget.complete_request();
                }
            }
            ProviderEvent::Cancelled { .. } => {
                request.phase = RequestPhase::Terminated;
                self.messages.push(Message {
                    role: Role::System,
                    content: "Request cancelled.".to_owned(),
                });
                if let Some(budget) = &mut self.context_budget {
                    budget.cancel_request();
                }
            }
            ProviderEvent::Failed { error, .. } => {
                request.phase = RequestPhase::Terminated;
                self.messages.push(Message {
                    role: Role::System,
                    content: format!("Request failed: {error}"),
                });
                if let Some(budget) = &mut self.context_budget {
                    budget.cancel_request();
                }
            }
        }
    }

    /// Removes all conversation history and any in-flight request tracking.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_activities.clear();
        self.request = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Role};
    use crate::event::ProviderEvent;

    fn delta(request_id: &str, text: &str) -> ProviderEvent {
        ProviderEvent::Delta {
            request_id: request_id.into(),
            text: text.into(),
        }
    }

    #[test]
    fn default_app_is_empty() {
        let app = App::default();
        assert!(app.messages.is_empty());
    }

    #[test]
    fn start_request_records_user_prompt() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());

        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, Role::User);
        assert_eq!(app.messages[0].content, "hello");
    }

    #[test]
    fn deltas_accumulate_into_one_assistant_message() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.apply_provider_event(ProviderEvent::Started {
            request_id: "r1".into(),
        });
        app.apply_provider_event(delta("r1", "Hi "));
        app.apply_provider_event(delta("r1", "there"));
        app.apply_provider_event(ProviderEvent::Completed {
            request_id: "r1".into(),
        });

        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[1].role, Role::Assistant);
        assert_eq!(app.messages[1].content, "Hi there");
    }

    #[test]
    fn late_deltas_after_cancellation_are_dropped() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.apply_provider_event(ProviderEvent::Cancelled {
            request_id: "r1".into(),
        });
        app.apply_provider_event(delta("r1", "late"));

        assert!(!app.messages.iter().any(|m| m.content.contains("late")));
        assert!(app.messages.iter().any(|m| m.role == Role::System));
    }

    #[test]
    fn late_deltas_after_completion_are_dropped() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.apply_provider_event(delta("r1", "done"));
        app.apply_provider_event(ProviderEvent::Completed {
            request_id: "r1".into(),
        });
        app.apply_provider_event(delta("r1", "late"));

        assert_eq!(app.messages[1].content, "done");
    }

    #[test]
    fn events_for_other_requests_are_ignored() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.apply_provider_event(delta("r2", "stranger"));

        assert_eq!(app.messages.len(), 1);
    }

    #[test]
    fn failure_records_a_system_message() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.apply_provider_event(ProviderEvent::Failed {
            request_id: "r1".into(),
            error: "boom".into(),
        });

        assert!(
            app.messages
                .iter()
                .any(|m| m.role == Role::System && m.content.contains("boom"))
        );
    }

    #[test]
    fn clear_resets_state() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.clear();

        assert!(app.messages.is_empty());
    }
}
