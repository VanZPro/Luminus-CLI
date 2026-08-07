use crate::agent::{AgentRun, AgentStatus};
use crate::context::ContextBudget;
use crate::event::ProviderEvent;
use crate::session::{SavedMessage, Session};
use crate::tool_activity::ToolActivity;
use crate::tool_event::{ToolCallId, ToolLifecycleEvent};
use crate::tool_output::{BoundedOutput, Bounds, TruncationKind};
use crate::tools::{ApprovalRequest, ToolError, ToolOutput};
use std::fmt;
use std::time::Duration;

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

impl From<&Message> for SavedMessage {
    fn from(message: &Message) -> Self {
        Self {
            role: message.role.to_string(),
            content: message.content.clone(),
        }
    }
}

impl Role {
    pub fn from_saved(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        })
    }
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
    Approval,
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
    /// Tracked spawned agents, newest last.
    pub agent_runs: Vec<AgentRun>,
    /// Pending coding tool that requires operator approval.
    pub pending_approval: Option<ApprovalRequest>,
}

impl App {
    /// Show the approval overlay for a tool invocation.
    pub fn show_approval(&mut self, request: ApprovalRequest) {
        self.pending_approval = Some(request);
        self.ui_mode = UiMode::Approval;
    }

    /// Accept the pending tool approval and clear the overlay.
    pub fn take_approval(&mut self) -> Option<ApprovalRequest> {
        self.ui_mode = UiMode::Normal;
        self.pending_approval.take()
    }

    /// Reject the pending tool approval and clear the overlay.
    ///
    /// Returns the rejected request when one was pending so callers can emit a
    /// matching [`ToolLifecycleEvent::Cancelled`] with the tool name.
    pub fn reject_approval(&mut self) -> Option<ApprovalRequest> {
        self.ui_mode = UiMode::Normal;
        self.pending_approval.take()
    }

    /// Append a raw tool output message to the conversation (unbounded).
    pub fn append_tool_output(&mut self, output: &ToolOutput) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: format!("tool {}:\n{}", output.tool, output.output),
        });
    }

    /// Append a bounded tool-output transcript line with call id + permission.
    ///
    /// When `bounded.truncated` is true the message includes total byte/line
    /// counts, omission counts, truncation kind, and notes that the full
    /// output is retained in-memory only (no disk artifact yet).
    pub fn append_bounded_tool_output(
        &mut self,
        id: ToolCallId,
        tool: &str,
        permission: &str,
        bounded: &BoundedOutput,
    ) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: format_bounded_tool_message(id, tool, permission, bounded),
        });
    }

    /// Push a [`ToolActivity`] card derived from a lifecycle event.
    ///
    /// Optional `duration` is attached to the activity card when provided
    /// (typically on terminal events).
    pub fn apply_tool_lifecycle(&mut self, event: &ToolLifecycleEvent, duration: Option<Duration>) {
        let mut activity = match event {
            ToolLifecycleEvent::Started { tool, .. } => ToolActivity::started(tool.clone()),
            ToolLifecycleEvent::Progress { tool, message, .. } => {
                ToolActivity::progress(tool.clone(), message.clone())
            }
            ToolLifecycleEvent::Completed { tool, output, .. } => {
                ToolActivity::completed(tool.clone(), output.clone())
            }
            ToolLifecycleEvent::Failed { tool, error, .. } => {
                ToolActivity::failed(tool.clone(), error.clone())
            }
            ToolLifecycleEvent::Cancelled { tool, .. } => {
                ToolActivity::failed(tool.clone(), "cancelled")
            }
        };
        if let Some(duration) = duration {
            activity = activity.with_duration(duration);
        }
        self.tool_activities.push(activity);
    }

    /// Emit [`ToolLifecycleEvent::Started`] and push a started activity card.
    ///
    /// Call this **before** `ToolRegistry::execute` so the lifecycle stream
    /// begins prior to side effects.
    pub fn begin_tool(&mut self, id: ToolCallId, tool: impl Into<String>) -> ToolLifecycleEvent {
        let event = ToolLifecycleEvent::Started {
            id,
            tool: tool.into(),
        };
        self.apply_tool_lifecycle(&event, None);
        event
    }

    /// Record the terminal outcome of an approved tool execution.
    ///
    /// Expects [`Self::begin_tool`] to have already been called with the same
    /// `id`. On success: bounds the output with [`Bounds::default`], appends a
    /// transcript message (with truncation metadata when needed), and emits
    /// Completed. On error: emits Failed. Returns a one-line command-log summary.
    pub fn record_tool_result(
        &mut self,
        id: ToolCallId,
        approval: &ApprovalRequest,
        result: Result<ToolOutput, ToolError>,
        duration: Duration,
    ) -> String {
        let tool = approval.request.name.as_str();
        let permission = approval.spec.permission.label();

        match result {
            Ok(output) => {
                let bounded = BoundedOutput::truncate(&output.output, Bounds::default());
                self.append_bounded_tool_output(id, tool, permission, &bounded);
                self.apply_tool_lifecycle(
                    &ToolLifecycleEvent::Completed {
                        id,
                        tool: tool.to_owned(),
                        output: bounded.preview.clone(),
                    },
                    Some(duration),
                );
                format_tool_summary_ok(id, tool, permission, &bounded)
            }
            Err(error) => {
                let error_text = error.to_string();
                self.apply_tool_lifecycle(
                    &ToolLifecycleEvent::Failed {
                        id,
                        tool: tool.to_owned(),
                        error: error_text.clone(),
                    },
                    Some(duration),
                );
                format!("tool {id} ({tool}): error: {error_text}")
            }
        }
    }

    /// Record a rejected/cancelled tool approval.
    ///
    /// Allocates a fresh [`ToolCallId`], emits [`ToolLifecycleEvent::Cancelled`],
    /// and pushes a cancelled activity card. Returns a summary line for the
    /// command log.
    pub fn record_tool_rejection(&mut self, tool: impl Into<String>) -> String {
        let tool = tool.into();
        let id = ToolCallId::new();
        let event = ToolLifecycleEvent::Cancelled {
            id,
            tool: tool.clone(),
        };
        self.apply_tool_lifecycle(&event, None);
        format!("tool {id} ({tool}): rejected")
    }

    /// Log a local command outcome without clearing tool activity cards.
    ///
    /// Unlike [`Self::start_request`], this preserves `tool_activities` so a
    /// just-recorded tool lifecycle remains visible in the TUI.
    pub fn start_command(&mut self, request_id: impl Into<String>, message: impl Into<String>) {
        self.messages.push(Message {
            role: Role::User,
            content: message.into(),
        });
        self.request = Some(ActiveRequest {
            request_id: request_id.into(),
            phase: RequestPhase::Streaming,
            message_index: None,
        });
    }

    pub fn snapshot_session(&self, name: impl Into<String>) -> Session {
        Session::new(name, self.messages.iter().map(SavedMessage::from).collect())
    }

    pub fn restore_session(&mut self, session: &Session) {
        self.clear();
        self.messages = session
            .messages
            .iter()
            .filter_map(|message| {
                Role::from_saved(&message.role).map(|role| Message {
                    role,
                    content: message.content.clone(),
                })
            })
            .collect();
    }

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

    /// Starts one child-agent run. Phase 9 permits only one active child.
    pub fn start_agent(&mut self, agent_id: String, request_id: String, prompt: String) -> bool {
        if self
            .agent_runs
            .iter()
            .any(|run| run.status == AgentStatus::Running)
        {
            return false;
        }
        self.agent_runs
            .push(AgentRun::new(agent_id, request_id, prompt));
        true
    }

    /// Routes an event to a child agent, returning whether it matched one.
    pub fn apply_agent_event(&mut self, event: &ProviderEvent) -> bool {
        self.agent_runs
            .iter_mut()
            .find(|run| run.request_id == event.request_id())
            .is_some_and(|run| run.apply_event(event))
    }

    /// Returns the currently running child-agent request, if any.
    pub fn active_agent_request_id(&self) -> Option<&str> {
        self.agent_runs
            .iter()
            .find(|run| run.status == AgentStatus::Running)
            .map(|run| run.request_id.as_str())
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

    /// Removes all conversation history, in-flight request tracking, and any
    /// transient UI/approval state so a stale approval overlay cannot survive
    /// a reset.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_activities.clear();
        self.agent_runs.clear();
        self.request = None;
        self.pending_approval = None;
        self.ui_mode = UiMode::Normal;
    }
}

/// Format a multi-line transcript message for a bounded tool result.
fn format_bounded_tool_message(
    id: ToolCallId,
    tool: &str,
    permission: &str,
    bounded: &BoundedOutput,
) -> String {
    let mut message = format!("tool {id} ({tool}, {permission}):\n{}", bounded.preview);
    if bounded.truncated {
        message.push_str(&format!(
            "\n\n[truncated: {} | total_bytes={} total_lines={} bytes_omitted={} lines_omitted={} | full_output available in-memory only (no disk artifact yet)]",
            truncation_kind_label(bounded.truncation),
            bounded.total_bytes,
            bounded.total_lines,
            bounded.bytes_omitted,
            bounded.lines_omitted,
        ));
    }
    message
}

/// Compact one-line summary for the command log / start_command prompt.
fn format_tool_summary_ok(
    id: ToolCallId,
    tool: &str,
    permission: &str,
    bounded: &BoundedOutput,
) -> String {
    let preview_one_line = bounded.preview.replace('\n', " ");
    if bounded.truncated {
        format!(
            "tool {id} ({tool}, {permission}): {preview_one_line} [truncated: {} | {}B/{}L omitted {}B/{}L; full_output in-memory only]",
            truncation_kind_label(bounded.truncation),
            bounded.total_bytes,
            bounded.total_lines,
            bounded.bytes_omitted,
            bounded.lines_omitted,
        )
    } else {
        format!("tool {id} ({tool}, {permission}): {preview_one_line}")
    }
}

fn truncation_kind_label(kind: TruncationKind) -> &'static str {
    match kind {
        TruncationKind::None => "none",
        TruncationKind::Bytes => "bytes",
        TruncationKind::Lines => "lines",
        TruncationKind::Both => "bytes+lines",
    }
}

#[cfg(test)]
mod tests {
    use super::{App, Role};
    use crate::event::ProviderEvent;
    use crate::tool_activity::ToolActivity;
    use crate::tool_event::{ToolCallId, ToolLifecycleEvent};
    use crate::tool_output::{BoundedOutput, Bounds, TruncationKind};
    use crate::tools::{ApprovalRequest, Permission, ToolError, ToolOutput, ToolRequest, ToolSpec};
    use std::time::Duration;

    fn delta(request_id: &str, text: &str) -> ProviderEvent {
        ProviderEvent::Delta {
            request_id: request_id.into(),
            text: text.into(),
        }
    }

    fn sample_approval(name: &str, permission: Permission) -> ApprovalRequest {
        ApprovalRequest {
            request: ToolRequest {
                name: name.into(),
                args: vec!["arg".into()],
            },
            spec: ToolSpec {
                name: "read_file",
                description: "read",
                permission,
            },
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
    fn approval_can_be_accepted_or_rejected() {
        use crate::app::UiMode;
        let approval = sample_approval("read_file", Permission::ReadOnly);
        let mut app = App::default();
        app.show_approval(approval.clone());
        assert_eq!(app.ui_mode(), UiMode::Approval);
        assert_eq!(app.take_approval(), Some(approval));
        assert_eq!(app.ui_mode(), UiMode::Normal);

        app.show_approval(sample_approval("run_shell", Permission::Execute));
        let rejected = app.reject_approval();
        assert!(rejected.is_some());
        assert_eq!(app.ui_mode(), UiMode::Normal);
        assert!(app.pending_approval.is_none());
    }

    #[test]
    fn clear_resets_state() {
        let mut app = App::default();
        app.start_request("r1".into(), "hello".into());
        app.clear();

        assert!(app.messages.is_empty());
        assert!(app.pending_approval.is_none());
    }

    #[test]
    fn append_bounded_tool_output_includes_id_permission_and_preview() {
        let mut app = App::default();
        let id = ToolCallId::new();
        let bounded = BoundedOutput::truncate("hello world", Bounds::default());
        app.append_bounded_tool_output(id, "read_file", "read-only", &bounded);

        assert_eq!(app.messages.len(), 1);
        let content = &app.messages[0].content;
        assert!(content.contains(&format!("tool {id}")));
        assert!(content.contains("read_file"));
        assert!(content.contains("read-only"));
        assert!(content.contains("hello world"));
        assert!(!content.contains("truncated"));
    }

    #[test]
    fn append_bounded_tool_output_reports_truncation_metadata() {
        let mut app = App::default();
        let id = ToolCallId::new();
        let input = "line1\nline2\nline3\nline4";
        let bounds = Bounds {
            max_bytes: None,
            max_lines: Some(2),
        };
        let bounded = BoundedOutput::truncate(input, bounds);
        assert!(bounded.truncated);
        assert_eq!(bounded.truncation, TruncationKind::Lines);

        app.append_bounded_tool_output(id, "run_shell", "execute", &bounded);
        let content = &app.messages[0].content;
        assert!(content.contains("line1\nline2"));
        assert!(content.contains("truncated"));
        assert!(content.contains("lines"));
        assert!(content.contains(&format!("total_bytes={}", bounded.total_bytes)));
        assert!(content.contains(&format!("total_lines={}", bounded.total_lines)));
        assert!(content.contains(&format!("bytes_omitted={}", bounded.bytes_omitted)));
        assert!(content.contains(&format!("lines_omitted={}", bounded.lines_omitted)));
        assert!(content.contains("full_output available in-memory only"));
        assert!(content.contains("no disk artifact yet"));
        assert!(bounded.full_output.is_some());
    }

    #[test]
    fn begin_and_record_tool_result_emit_started_then_completed() {
        let mut app = App::default();
        let id = ToolCallId::new();
        let approval = sample_approval("read_file", Permission::ReadOnly);

        app.begin_tool(id, "read_file");
        assert_eq!(app.tool_activities.len(), 1);
        assert!(matches!(
            &app.tool_activities[0],
            ToolActivity::Started { .. }
        ));

        let summary = app.record_tool_result(
            id,
            &approval,
            Ok(ToolOutput {
                tool: "read_file".into(),
                output: "file contents".into(),
            }),
            Duration::from_millis(12),
        );

        assert!(summary.contains(&format!("tool {id}")));
        assert!(summary.contains("read_file"));
        assert!(summary.contains("read-only"));
        assert!(summary.contains("file contents"));
        assert_eq!(app.tool_activities.len(), 2);
        assert!(matches!(
            &app.tool_activities[1],
            ToolActivity::Completed { meta, output }
                if meta.duration == Some(Duration::from_millis(12))
                    && output == "file contents"
        ));
        assert!(
            app.messages
                .iter()
                .any(|m| m.role == Role::Assistant && m.content.contains("file contents"))
        );
    }

    #[test]
    fn record_tool_result_emits_failed_on_error() {
        let mut app = App::default();
        let id = ToolCallId::new();
        let approval = sample_approval("http_get", Permission::Network);
        app.begin_tool(id, "http_get");
        let summary = app.record_tool_result(
            id,
            &approval,
            Err(ToolError::NetworkDisabled),
            Duration::from_millis(1),
        );

        assert!(summary.contains("error"));
        assert!(summary.contains("network tools are disabled"));
        assert!(matches!(
            app.tool_activities.last(),
            Some(ToolActivity::Failed { error, .. })
                if error.contains("network tools are disabled")
        ));
    }

    #[test]
    fn record_tool_rejection_emits_cancelled_activity() {
        let mut app = App::default();
        let summary = app.record_tool_rejection("write_file");
        assert!(summary.contains("rejected"));
        assert!(summary.contains("write_file"));
        assert!(matches!(
            app.tool_activities.last(),
            Some(ToolActivity::Failed { error, .. }) if error == "cancelled"
        ));
    }

    #[test]
    fn start_command_preserves_tool_activities() {
        let mut app = App::default();
        let id = ToolCallId::new();
        app.begin_tool(id, "list_dir");
        assert_eq!(app.tool_activities.len(), 1);

        app.start_command("command", "tool finished");
        assert_eq!(app.tool_activities.len(), 1);
        assert_eq!(
            app.messages.last().map(|m| m.content.as_str()),
            Some("tool finished")
        );
    }

    #[test]
    fn clear_still_clears_tool_activities() {
        let mut app = App::default();
        let id = ToolCallId::new();
        app.begin_tool(id, "list_dir");
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Completed {
                id,
                tool: "list_dir".into(),
                output: "ok".into(),
            },
            Some(Duration::from_millis(5)),
        );
        assert!(!app.tool_activities.is_empty());

        app.clear();
        assert!(app.tool_activities.is_empty());
        assert!(app.messages.is_empty());
        assert!(app.pending_approval.is_none());
    }

    #[test]
    fn apply_tool_lifecycle_maps_all_variants() {
        let mut app = App::default();
        let id = ToolCallId::new();
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Started {
                id,
                tool: "t".into(),
            },
            None,
        );
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Progress {
                id,
                tool: "t".into(),
                message: "working".into(),
            },
            None,
        );
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Completed {
                id,
                tool: "t".into(),
                output: "done".into(),
            },
            Some(Duration::from_millis(3)),
        );
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Failed {
                id,
                tool: "t".into(),
                error: "err".into(),
            },
            None,
        );
        app.apply_tool_lifecycle(
            &ToolLifecycleEvent::Cancelled {
                id,
                tool: "t".into(),
            },
            None,
        );
        assert_eq!(app.tool_activities.len(), 5);
        assert!(matches!(
            app.tool_activities[0],
            ToolActivity::Started { .. }
        ));
        assert!(matches!(
            app.tool_activities[1],
            ToolActivity::Progress { .. }
        ));
        assert!(matches!(
            app.tool_activities[2],
            ToolActivity::Completed { .. }
        ));
        assert!(matches!(
            app.tool_activities[3],
            ToolActivity::Failed { .. }
        ));
        assert!(matches!(
            &app.tool_activities[4],
            ToolActivity::Failed { error, .. } if error == "cancelled"
        ));
    }
}
