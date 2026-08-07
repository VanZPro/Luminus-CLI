use crate::agent::{AgentRun, AgentStatus};
use crate::artifact_store::ArtifactStore;
use crate::context::ContextBudget;
use crate::diff_history::DiffHistory;
use crate::event::ProviderEvent;
use crate::permission_policy::{ProjectToolPolicy, ToolPolicy};
use crate::session::{SavedMessage, Session, SessionEvent};
use crate::skill::SkillRegistry;
use crate::tool_activity::ToolActivity;
use crate::tool_event::{ToolCallId, ToolLifecycleEvent};
use crate::tool_output::{BoundedOutput, Bounds, TruncationKind};
use crate::tools::{ApprovalRequest, ToolError, ToolOutput};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// Operator choice on a pending tool-approval overlay (Intruksi / Hermes-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalChoice {
    /// Approve this invocation only.
    AllowOnce,
    /// Approve this invocation and auto-allow matching tool for the rest of the process.
    AllowSession,
    /// Approve this invocation and persist allow for the tool in the project policy file.
    AllowProject,
    /// Reject this invocation only.
    Reject,
    /// Reject this invocation and auto-deny matching tool for the rest of the process.
    RejectDenySession,
    /// Reject this invocation and persist deny for the tool in the project policy file.
    RejectDenyProject,
}

impl ApprovalChoice {
    /// Stable string label for session event logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowSession => "allow_session",
            Self::AllowProject => "allow_project",
            Self::Reject => "reject",
            Self::RejectDenySession => "deny_session",
            Self::RejectDenyProject => "deny_project",
        }
    }
}

/// Session-scoped policy for a tool name (process lifetime, cleared on [`App::clear`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionToolPolicy {
    Allowed,
    Denied,
}

/// Result of routing a prepared approval through session + project allow/deny lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalGate {
    /// Tool is on the session allowlist — execute without showing the overlay.
    SessionAllowed(ApprovalRequest),
    /// Tool is on the session denylist — skip overlay and surface a denial.
    SessionDenied { tool: String },
    /// Tool is on the project allowlist — execute without showing the overlay.
    ProjectAllowed(ApprovalRequest),
    /// Tool is on the project denylist — skip overlay and surface a denial.
    ProjectDenied { tool: String },
    /// No session or project policy matched; the approval overlay is now active.
    NeedsPrompt,
}

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
    DiffView,
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
    /// Session-scoped allow/deny policy keyed by tool name (process lifetime).
    session_tool_policy: HashMap<String, SessionToolPolicy>,
    /// Project-persisted allow/deny policy (`.luminus/tool_policy.json`).
    ///
    /// Loaded at startup via [`Self::load_project_policy`]. Survives [`Self::clear`].
    project_tool_policy: Option<ProjectToolPolicy>,
    /// Last non-fatal error from a project-policy disk write (surface to the UI).
    pub last_policy_error: Option<String>,
    /// Event-oriented session log (tools/approvals); dual-written on snapshot.
    session_events: Vec<SessionEvent>,
    /// Disk store for full truncated tool outputs (`<data_root>/artifacts/`).
    artifact_store: ArtifactStore,
    /// File edit history with undo/redo stacks.
    diff_history: DiffHistory,
    /// Skill registry (built-in + global + project skills).
    skill_registry: SkillRegistry,
}

impl App {
    /// Current session policy for `tool`, if any.
    pub fn session_policy(&self, tool: &str) -> Option<SessionToolPolicy> {
        self.session_tool_policy.get(tool).copied()
    }

    /// Current project policy for `tool`, if a project store is loaded and has an entry.
    pub fn project_policy(&self, tool: &str) -> Option<ToolPolicy> {
        self.project_tool_policy.as_ref()?.get(tool)
    }

    /// Borrow the loaded project policy store, if any.
    pub fn project_tool_policy(&self) -> Option<&ProjectToolPolicy> {
        self.project_tool_policy.as_ref()
    }

    /// Load (or replace) project tool policy from `project_root/.luminus/tool_policy.json`.
    ///
    /// Missing file is fine (empty in-memory store bound to that root). I/O
    /// failures return `Err` and leave any previous store untouched.
    pub fn load_project_policy(
        &mut self,
        project_root: impl AsRef<Path>,
    ) -> std::io::Result<&ProjectToolPolicy> {
        let policy = ProjectToolPolicy::load(project_root)?;
        self.project_tool_policy = Some(policy);
        Ok(self
            .project_tool_policy
            .as_ref()
            .expect("just inserted project policy"))
    }

    /// Load project policy from the process current working directory.
    ///
    /// On failure, records a message in [`Self::last_policy_error`] and returns
    /// `false` without panicking.
    pub fn load_project_policy_from_cwd(&mut self) -> bool {
        let root = match std::env::current_dir() {
            Ok(root) => root,
            Err(error) => {
                self.last_policy_error =
                    Some(format!("could not resolve project directory: {error}"));
                return false;
            }
        };
        match self.load_project_policy(&root) {
            Ok(_) => {
                self.last_policy_error = None;
                true
            }
            Err(error) => {
                self.last_policy_error = Some(format!(
                    "could not load project tool policy from {}: {error}",
                    root.display()
                ));
                false
            }
        }
    }

    /// Route a prepared approval through session **and** project allow/deny lists.
    ///
    /// Precedence (first match wins):
    /// 1. session deny
    /// 2. project deny
    /// 3. session allow
    /// 4. project allow
    /// 5. prompt (overlay)
    pub fn gate_approval(&mut self, request: ApprovalRequest) -> ApprovalGate {
        let tool = request.request.name.as_str();
        if self.session_policy(tool) == Some(SessionToolPolicy::Denied) {
            return ApprovalGate::SessionDenied {
                tool: tool.to_owned(),
            };
        }
        if self.project_policy(tool) == Some(ToolPolicy::Denied) {
            return ApprovalGate::ProjectDenied {
                tool: tool.to_owned(),
            };
        }
        if self.session_policy(tool) == Some(SessionToolPolicy::Allowed) {
            return ApprovalGate::SessionAllowed(request);
        }
        if self.project_policy(tool) == Some(ToolPolicy::Allowed) {
            return ApprovalGate::ProjectAllowed(request);
        }
        self.show_approval(request);
        ApprovalGate::NeedsPrompt
    }

    /// Show the approval overlay for a tool invocation.
    ///
    /// Prefer [`Self::gate_approval`] at call sites so session/project allow/deny
    /// lists are honoured; this method forces the overlay regardless of policy.
    pub fn show_approval(&mut self, request: ApprovalRequest) {
        self.pending_approval = Some(request);
        self.ui_mode = UiMode::Approval;
    }

    /// Apply an operator choice to the pending approval and clear the overlay.
    ///
    /// Session choices update the process-local allow/deny map (keyed by tool
    /// name). Project choices update the in-memory project store **and** attempt
    /// an atomic disk save; save failures set [`Self::last_policy_error`] and do
    /// not panic. Once-only choices leave both maps unchanged.
    pub fn resolve_approval(&mut self, choice: ApprovalChoice) -> Option<ApprovalRequest> {
        self.ui_mode = UiMode::Normal;
        let approval = self.pending_approval.take()?;
        let tool = approval.request.name.clone();
        match choice {
            ApprovalChoice::AllowOnce | ApprovalChoice::Reject => {}
            ApprovalChoice::AllowSession => {
                self.session_tool_policy
                    .insert(tool.clone(), SessionToolPolicy::Allowed);
            }
            ApprovalChoice::RejectDenySession => {
                self.session_tool_policy
                    .insert(tool.clone(), SessionToolPolicy::Denied);
            }
            ApprovalChoice::AllowProject => {
                self.persist_project_policy(&tool, ToolPolicy::Allowed);
            }
            ApprovalChoice::RejectDenyProject => {
                self.persist_project_policy(&tool, ToolPolicy::Denied);
            }
        }
        self.session_events.push(SessionEvent::ApprovalResolved {
            tool,
            choice: choice.as_str().to_owned(),
        });
        Some(approval)
    }

    /// Update in-memory project policy and save to disk. Failures are recorded
    /// in [`Self::last_policy_error`] rather than panicking.
    fn persist_project_policy(&mut self, tool: &str, policy: ToolPolicy) {
        if self.project_tool_policy.is_none() {
            // Ensure we have a store bound to cwd so P/X still work mid-session.
            let root = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
            self.project_tool_policy = Some(ProjectToolPolicy::empty(root));
        }
        if let Some(store) = self.project_tool_policy.as_mut() {
            store.set(tool, policy);
            match store.save() {
                Ok(_) => self.last_policy_error = None,
                Err(error) => {
                    self.last_policy_error = Some(format!(
                        "failed to save project tool policy for `{tool}`: {error}"
                    ));
                }
            }
        }
    }

    /// Accept the pending tool approval once and clear the overlay.
    ///
    /// Equivalent to [`Self::resolve_approval`] with [`ApprovalChoice::AllowOnce`].
    pub fn take_approval(&mut self) -> Option<ApprovalRequest> {
        self.resolve_approval(ApprovalChoice::AllowOnce)
    }

    /// Reject the pending tool approval once and clear the overlay.
    ///
    /// Equivalent to [`Self::resolve_approval`] with [`ApprovalChoice::Reject`].
    /// Returns the rejected request when one was pending so callers can emit a
    /// matching [`ToolLifecycleEvent::Cancelled`] with the tool name.
    pub fn reject_approval(&mut self) -> Option<ApprovalRequest> {
        self.resolve_approval(ApprovalChoice::Reject)
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
    /// counts, omission counts, truncation kind, and any persisted artifact id.
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
        let tool = tool.into();
        let event = ToolLifecycleEvent::Started {
            id,
            tool: tool.clone(),
        };
        self.apply_tool_lifecycle(&event, None);
        self.session_events.push(SessionEvent::ToolStarted {
            id: id.to_string(),
            tool,
        });
        event
    }

    /// Record the terminal outcome of an approved tool execution.
    ///
    /// Expects [`Self::begin_tool`] to have already been called with the same
    /// `id`. On success: bounds the output with [`Bounds::default`], persists
    /// truncated full output to the artifact store when possible, appends a
    /// transcript message, and emits Completed. On error: emits Failed.
    /// Returns a one-line command-log summary.
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
                let mut bounded = BoundedOutput::truncate(&output.output, Bounds::default());
                if let Err(error) = bounded.persist_if_truncated(&self.artifact_store) {
                    self.last_policy_error = Some(format!(
                        "failed to persist tool artifact for `{tool}`: {error}"
                    ));
                }
                self.append_bounded_tool_output(id, tool, permission, &bounded);
                let summary = format_tool_summary_ok(id, tool, permission, &bounded);
                self.apply_tool_lifecycle(
                    &ToolLifecycleEvent::Completed {
                        id,
                        tool: tool.to_owned(),
                        output: bounded.preview.clone(),
                    },
                    Some(duration),
                );
                self.session_events.push(SessionEvent::ToolCompleted {
                    id: id.to_string(),
                    tool: tool.to_owned(),
                    ok: true,
                    summary: summary.clone(),
                });
                summary
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
                self.session_events.push(SessionEvent::ToolFailed {
                    id: id.to_string(),
                    tool: tool.to_owned(),
                    error: error_text.clone(),
                });
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
        self.session_events.push(SessionEvent::ToolCancelled {
            id: id.to_string(),
            tool: tool.clone(),
            reason: "rejected".into(),
        });
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
        let messages: Vec<SavedMessage> = self.messages.iter().map(SavedMessage::from).collect();
        let mut session = Session::new(name, messages);
        if self.session_events.is_empty() {
            // Dual-write message events so the event log is usable on first save.
            let message_events: Vec<SessionEvent> = session
                .messages
                .iter()
                .map(|message| SessionEvent::Message {
                    role: message.role.clone(),
                    content: message.content.clone(),
                })
                .collect();
            for event in message_events {
                session.append_event(event);
            }
        } else {
            session.events = self.session_events.clone();
        }
        session
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
        self.session_events = session.events.clone();
        if self.session_events.is_empty() {
            // Legacy sessions: synthesize message events from transcript.
            for message in &self.messages {
                self.session_events.push(SessionEvent::Message {
                    role: message.role.to_string(),
                    content: message.content.clone(),
                });
            }
        }
    }

    /// Borrow the in-memory session event log (tools/approvals).
    pub fn session_events(&self) -> &[SessionEvent] {
        &self.session_events
    }

    /// Borrow the artifact store used for truncated tool full-output.
    pub fn artifact_store(&self) -> &ArtifactStore {
        &self.artifact_store
    }

    /// Current UI mode.
    pub fn ui_mode(&self) -> UiMode {
        self.ui_mode
    }

    /// Borrow the file edit history.
    pub fn diff_history(&self) -> &DiffHistory {
        &self.diff_history
    }

    /// Borrow the file edit history mutably.
    pub fn diff_history_mut(&mut self) -> &mut DiffHistory {
        &mut self.diff_history
    }

    /// Show the diff viewer overlay.
    pub fn show_diff_view(&mut self) {
        self.ui_mode = UiMode::DiffView;
    }

    /// Hide the diff viewer overlay.
    pub fn hide_diff_view(&mut self) {
        self.ui_mode = UiMode::Normal;
    }

    /// Handle `/changes`: produce a summary of modified paths with line counts.
    pub fn handle_changes(&self) -> String {
        let changes = self.diff_history.changes();
        if changes.is_empty() {
            return "No changes recorded.".to_owned();
        }
        let mut lines = vec!["Changes:".to_owned()];
        for (path, added, removed) in changes {
            lines.push(format!("  {} (+{} -{})", path.display(), added, removed));
        }
        lines.join("\n")
    }

    /// Handle `/undo`: undo the most recent edit and return a summary message.
    pub fn handle_undo(&mut self) -> String {
        match self.diff_history.undo() {
            Some(record) => {
                format!(
                    "Undid edit: {} (reverted to before-state)",
                    record.path.display()
                )
            }
            None => "Nothing to undo.".to_owned(),
        }
    }

    /// Handle `/redo`: redo the most recently undone edit and return a summary.
    pub fn handle_redo(&mut self) -> String {
        match self.diff_history.redo() {
            Some(record) => {
                format!(
                    "Redid edit: {} (reapplied after-state)",
                    record.path.display()
                )
            }
            None => "Nothing to redo.".to_owned(),
        }
    }

    /// Handle `/revert-file`: revert a file to its initial state.
    pub fn handle_revert_file(&mut self, path: &str) -> String {
        let path = std::path::PathBuf::from(path);
        match self.diff_history.revert_file(&path) {
            Some(record) => {
                format!("Reverted {} to its initial state", record.path.display())
            }
            None => format!("No edit history for: {path}", path = path.display()),
        }
    }

    /// Handle `/skills` or `/skills list`: discover and list skills.
    pub fn handle_skills_list(&self) -> String {
        let items = self.skill_registry.list_skills();
        if items.is_empty() {
            "No skills found. Add skills to .luminus/skills/<name>/SKILL.md or ~/.config/luminus/skills/".to_owned()
        } else {
            items
        }
    }

    /// Handle `/skills inspect <name>`: show detailed skill metadata.
    pub fn handle_skill_inspect(&self, name: &str) -> String {
        match self.skill_registry.inspect_skill(name) {
            Ok(text) => text,
            Err(error) => format!("{error}"),
        }
    }

    /// Handle `/skill <name>`: load and activate a skill.
    pub fn handle_skill_use(&mut self, name: &str) -> String {
        match self.skill_registry.load_skill(name) {
            Ok(skill) => {
                // Inject skill instructions as a system message for context.
                self.messages.push(Message {
                    role: Role::System,
                    content: format!(
                        "[Skill activated: {} ({})]\n\n{}",
                        skill.metadata.name, skill.metadata.source, skill.content
                    ),
                });
                format!(
                    "Skill '{}' activated ({}). Instructions injected into context.",
                    skill.metadata.name, skill.metadata.source
                )
            }
            Err(error) => format!("{error}"),
        }
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
    /// a reset. Also clears **session-scoped** tool allow/deny lists.
    ///
    /// Project-persisted rules (`.luminus/tool_policy.json` and the in-memory
    /// [`ProjectToolPolicy`] store) are **not** cleared — they outlive `/clear`.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.tool_activities.clear();
        self.agent_runs.clear();
        self.request = None;
        self.pending_approval = None;
        self.ui_mode = UiMode::Normal;
        self.session_tool_policy.clear();
        self.last_policy_error = None;
        self.session_events.clear();
        // Project policy + artifact store survive /clear (disk-backed).
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
        let artifact_note = match bounded.artifact_id.as_ref() {
            Some(aid) => format!("full_output on disk artifact_id={aid}"),
            None if bounded.full_output.is_some() => {
                "full_output available in-memory only (no disk artifact yet)".to_owned()
            }
            None => "full_output unavailable (not persisted)".to_owned(),
        };
        message.push_str(&format!(
            "\n\n[truncated: {} | total_bytes={} total_lines={} bytes_omitted={} lines_omitted={} | {artifact_note}]",
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
        let artifact_note = match bounded.artifact_id.as_ref() {
            Some(aid) => format!("artifact_id={aid}"),
            None if bounded.full_output.is_some() => "full_output in-memory only".to_owned(),
            None => "full_output unavailable".to_owned(),
        };
        format!(
            "tool {id} ({tool}, {permission}): {preview_one_line} [truncated: {} | {}B/{}L omitted {}B/{}L; {artifact_note}]",
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
        // Without a store, truncated output still notes in-memory full text.
        assert!(
            content.contains("full_output available in-memory only")
                || content.contains("artifact_id=")
                || content.contains("full_output unavailable")
        );
        assert!(bounded.full_output.is_some() || bounded.artifact_id.is_some());
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
