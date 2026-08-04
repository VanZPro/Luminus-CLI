//! Spawn-agent domain types and helpers.
//!
//! A spawned agent is an independent provider request that runs alongside the
//! main conversation. It reuses the existing `Provider` trait and
//! `ProviderEvent` channel, but its lifecycle is tracked separately from the
//! main `App` request so the parent chat keeps streaming while the child runs.
//!
//! Phase 9 deliberately keeps the contract minimal: a single active agent at a
//! time, no recursion, no tool execution, no persistence. The reducer in
//! [`crate::app::App`] owns the [`AgentRun`] entries and routes events by
//! `request_id`.

use crate::event::ProviderEvent;

/// Lifecycle state of a spawned agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    /// The agent request has been started and is still streaming.
    Running,
    /// The provider emitted `Completed`; the agent finished successfully.
    Completed,
    /// The agent was cancelled before finishing.
    Cancelled,
    /// The provider emitted `Failed` with the given error message.
    Failed(String),
}

impl AgentStatus {
    /// Whether this status permanently ends the agent run.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed(_))
    }
}

/// One spawned agent run tracked by the application reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRun {
    /// Short identifier shown to the user (suffix of `request_id`).
    pub agent_id: String,
    /// Unique provider request id backing this agent.
    pub request_id: String,
    /// The prompt that launched the agent.
    pub prompt: String,
    /// Current lifecycle status.
    pub status: AgentStatus,
    /// Accumulated assistant text from `ProviderEvent::Delta`.
    pub output: String,
}

impl AgentRun {
    /// Create a freshly started running agent.
    pub fn new(agent_id: String, request_id: String, prompt: String) -> Self {
        Self {
            agent_id,
            request_id,
            prompt,
            status: AgentStatus::Running,
            output: String::new(),
        }
    }

    /// Apply a provider event targeted at this agent's `request_id`.
    ///
    /// Returns `true` if the event was consumed (matched this agent), `false`
    /// otherwise. Late deltas after a terminal status are ignored.
    pub fn apply_event(&mut self, event: &ProviderEvent) -> bool {
        if event.request_id() != self.request_id {
            return false;
        }
        if self.status.is_terminal() {
            return true;
        }
        match event {
            ProviderEvent::Started { .. }
            | ProviderEvent::ToolStarted { .. }
            | ProviderEvent::ToolProgress { .. }
            | ProviderEvent::ToolCompleted { .. }
            | ProviderEvent::ToolFailed { .. } => {}
            ProviderEvent::Delta { text, .. } => self.output.push_str(text),
            ProviderEvent::Completed { .. } => self.status = AgentStatus::Completed,
            ProviderEvent::Cancelled { .. } => self.status = AgentStatus::Cancelled,
            ProviderEvent::Failed { error, .. } => {
                self.status = AgentStatus::Failed(error.clone());
            }
        }
        true
    }
}

/// Produce a short human-readable id from a request id (last 8 chars).
pub fn short_id(request_id: &str) -> String {
    let len = request_id.len();
    if len <= 8 {
        request_id.to_owned()
    } else {
        request_id[len - 8..].to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentRun, AgentStatus, short_id};
    use crate::event::ProviderEvent;

    fn delta(request_id: &str, text: &str) -> ProviderEvent {
        ProviderEvent::Delta {
            request_id: request_id.into(),
            text: text.into(),
        }
    }

    #[test]
    fn new_agent_is_running_with_empty_output() {
        let run = AgentRun::new("abc12345".into(), "req-1".into(), "review code".into());
        assert_eq!(run.status, AgentStatus::Running);
        assert!(run.output.is_empty());
        assert_eq!(run.prompt, "review code");
    }

    #[test]
    fn deltas_accumulate_into_output() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        assert!(run.apply_event(&delta("req-1", "Hello ")));
        assert!(run.apply_event(&delta("req-1", "world")));
        assert_eq!(run.output, "Hello world");
        assert_eq!(run.status, AgentStatus::Running);
    }

    #[test]
    fn completed_marks_terminal() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        run.apply_event(&delta("req-1", "x"));
        run.apply_event(&ProviderEvent::Completed {
            request_id: "req-1".into(),
        });
        assert_eq!(run.status, AgentStatus::Completed);
        assert!(run.status.is_terminal());
    }

    #[test]
    fn failed_stores_error_message() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        run.apply_event(&ProviderEvent::Failed {
            request_id: "req-1".into(),
            error: "boom".into(),
        });
        assert_eq!(run.status, AgentStatus::Failed("boom".into()));
        assert!(run.status.is_terminal());
    }

    #[test]
    fn cancelled_marks_terminal() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        run.apply_event(&ProviderEvent::Cancelled {
            request_id: "req-1".into(),
        });
        assert_eq!(run.status, AgentStatus::Cancelled);
    }

    #[test]
    fn late_deltas_after_completion_are_ignored() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        run.apply_event(&delta("req-1", "done"));
        run.apply_event(&ProviderEvent::Completed {
            request_id: "req-1".into(),
        });
        assert!(run.apply_event(&delta("req-1", "late")));
        assert_eq!(run.output, "done");
    }

    #[test]
    fn event_for_other_request_is_ignored() {
        let mut run = AgentRun::new("abc12345".into(), "req-1".into(), "p".into());
        assert!(!run.apply_event(&delta("req-other", "nope")));
        assert!(run.output.is_empty());
        assert_eq!(run.status, AgentStatus::Running);
    }

    #[test]
    fn short_id_trims_long_ids() {
        assert_eq!(short_id("abcdefgh12345678"), "12345678");
        assert_eq!(short_id("short"), "short");
    }
}
