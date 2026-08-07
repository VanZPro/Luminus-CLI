use std::io::{self, stdout};
use std::time::Duration;

use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use luminus::{
    app::{App, ApprovalChoice, ApprovalGate},
    command::{self, Command, parse_command},
    event::ProviderEvent,
    model::{ModelCatalog, ModelRole, ModelSelection},
    provider::{FakeProvider, ModelDiscovery, Provider},
    providers::openai_runtime::{OpenAiProvider, RuntimeProvider},
    session::{Session, default_root},
    tool_event::ToolCallId,
    tools::{ApprovalRequest, ToolError, ToolOutput, ToolRegistry, ToolRequest},
    tui::{self, Theme},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "luminus", version, about = "Illuminate the codebase.")]
struct Cli {}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = Cli::parse();
    run_interactive().await
}

async fn run_interactive() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::default();
    // Project-persisted tool allow/deny from `<cwd>/.luminus/tool_policy.json`.
    // Failures are non-fatal (message on `last_policy_error`).
    let _ = app.load_project_policy_from_cwd();
    let mut composer = String::new();
    let mut cancel: Option<CancellationToken> = None;
    let mut agent_cancel: Option<CancellationToken> = None;
    let mut pending_tool: Option<PendingToolExec> = None;
    let (provider_tx, mut provider_rx) = mpsc::unbounded_channel::<ProviderEvent>();
    let mut provider = RuntimeProvider::from_env_or_fake(Duration::from_millis(80));
    let mut model_catalog = ModelCatalog::new();
    let tool_registry = ToolRegistry;
    let mut mcp_manager = luminus::mcp::McpManager::new();
    for (role, model) in [
        (ModelRole::Default, "fake-model"),
        (ModelRole::Fast, "fake-fast"),
        (ModelRole::Deep, "fake-deep"),
    ] {
        let _ = model_catalog.add(ModelSelection::new("fake", model, role));
    }
    let _ = model_catalog.select_role(ModelRole::Default);
    let monochrome = std::env::var_os("NO_COLOR").is_some();
    let mut should_exit = false;

    while !should_exit {
        terminal.draw(|frame| {
            tui::draw_with_composer(frame, &app, Theme::luminus(monochrome), &composer)
        })?;

        while let Ok(event) = provider_rx.try_recv() {
            if app.apply_agent_event(&event) {
                if event.is_terminal() {
                    agent_cancel = None;
                }
            } else {
                if event.is_terminal() {
                    cancel = None;
                }
                app.apply_provider_event(event);
            }
        }

        // Non-blocking poll for background tool completion (cancellable shell).
        if let Some(pending) = pending_tool.as_ref() {
            match pending.rx.try_recv() {
                Ok(result) => {
                    let finished = pending_tool.take().expect("pending tool just completed");
                    if cancel.as_ref().is_some_and(|t| t == &finished.cancel) {
                        cancel = None;
                    }
                    finish_tool_execution(&mut app, finished, result);
                }
                Err(std_mpsc::TryRecvError::Empty) => {}
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    let finished = pending_tool.take().expect("pending tool worker died");
                    if cancel.as_ref().is_some_and(|t| t == &finished.cancel) {
                        cancel = None;
                    }
                    finish_tool_execution(
                        &mut app,
                        finished,
                        Err(ToolError::Process("tool worker disconnected".into())),
                    );
                }
            }
        }

        if event::poll(Duration::from_millis(40))? {
            let ev = event::read()?;

            // Approval overlay intercepts keys while a tool is pending.
            // Keys (Intruksi / Hermes-style):
            //   Y / Enter — allow once
            //   A         — allow for session
            //   P         — allow + persist project
            //   N / Esc   — reject once
            //   D         — reject + deny for session
            //   X         — reject + persist project deny
            if app.ui_mode() == luminus::app::UiMode::Approval {
                if let Event::Key(KeyEvent {
                    code,
                    kind: event::KeyEventKind::Press,
                    ..
                }) = ev
                {
                    let choice = match code {
                        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                            Some(ApprovalChoice::AllowOnce)
                        }
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            Some(ApprovalChoice::AllowSession)
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            Some(ApprovalChoice::AllowProject)
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            Some(ApprovalChoice::Reject)
                        }
                        KeyCode::Char('d') | KeyCode::Char('D') => {
                            Some(ApprovalChoice::RejectDenySession)
                        }
                        KeyCode::Char('x') | KeyCode::Char('X') => {
                            Some(ApprovalChoice::RejectDenyProject)
                        }
                        _ => None,
                    };
                    if let Some(choice) = choice {
                        handle_approval_choice(
                            &mut app,
                            &tool_registry,
                            choice,
                            &mut cancel,
                            &mut pending_tool,
                        );
                    }
                }
                continue;
            }

            // Diff view overlay intercepts Esc to close.
            if app.ui_mode() == luminus::app::UiMode::DiffView {
                if let Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: event::KeyEventKind::Press,
                    ..
                }) = ev
                {
                    app.hide_diff_view();
                }
                continue;
            }

            // Model selector overlay intercepts keys while open.
            if app.ui_mode() == luminus::app::UiMode::ModelSelector {
                if let Event::Key(KeyEvent {
                    code,
                    kind: event::KeyEventKind::Press,
                    ..
                }) = ev
                {
                    match code {
                        KeyCode::Esc => app.hide_model_selector(),
                        KeyCode::Up => app.model_selector_prev(),
                        KeyCode::Down => app.model_selector_next(),
                        KeyCode::Enter => {
                            let selected_role = model_catalog
                                .list()
                                .get(app.model_selector_index)
                                .map(|sel| sel.role);
                            app.hide_model_selector();
                            if let Some(role) = selected_role {
                                let message = match model_catalog.select_role(role) {
                                    Ok(selection) => format!(
                                        "Model selected: {} / {}",
                                        selection.provider, selection.model
                                    ),
                                    Err(error) => error.to_string(),
                                };
                                app.start_request("command".into(), message);
                                app.apply_provider_event(ProviderEvent::Completed {
                                    request_id: "command".into(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }

            match ev {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(token) = &agent_cancel {
                        token.cancel();
                    } else if let Some(token) = &cancel {
                        token.cancel();
                    } else {
                        should_exit = true;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('m'),
                    modifiers,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    let items = model_catalog
                        .list()
                        .iter()
                        .map(|sel| {
                            let active = if model_catalog.active() == Some(sel) {
                                " (active)"
                            } else {
                                ""
                            };
                            format!("{} -> {} / {}{}", sel.role, sel.provider, sel.model, active)
                        })
                        .collect();
                    app.show_model_selector(items);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char('o'),
                    modifiers,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    app.show_diff_view();
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: event::KeyEventKind::Press,
                    ..
                }) => {
                    if let Some(token) = &agent_cancel {
                        token.cancel();
                    } else if let Some(token) = &cancel {
                        token.cancel();
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Up,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if composer.starts_with('/') => {
                    app.move_slash_autocomplete(-1);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if composer.starts_with('/') => {
                    app.move_slash_autocomplete(1);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Tab,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if composer.starts_with('/') => {
                    // Auto-complete the selected slash command.
                    let selected_index = app.slash_autocomplete_index;
                    const COMMANDS: &[&str] = &[
                        "/help",
                        "/about",
                        "/clear",
                        "/exit",
                        "/model",
                        "/models",
                        "/discover",
                        "/save",
                        "/sessions",
                        "/load",
                        "/tools",
                        "/tool",
                        "/provider",
                        "/spawn",
                        "/diff",
                        "/changes",
                        "/undo",
                        "/redo",
                        "/revert-file",
                        "/skills",
                        "/skill",
                        "/env",
                        "/mcp",
                    ];
                    let mut matches = COMMANDS
                        .iter()
                        .filter(|c| c.starts_with(composer.as_str()))
                        .copied()
                        .collect::<Vec<_>>();
                    if matches.is_empty() {
                        matches = COMMANDS.to_vec();
                    }
                    if let Some(selected) = matches.get(selected_index) {
                        composer = (*selected).to_string();
                        app.reset_slash_autocomplete();
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Tab,
                    kind: event::KeyEventKind::Press,
                    ..
                }) => {
                    // Normal tab: cycle agent mode
                    let new_mode = app.cycle_agent_mode();
                    app.start_command(
                        "command",
                        format!("Agent mode switched to: {}", new_mode.label()),
                    );
                    app.apply_provider_event(ProviderEvent::Completed {
                        request_id: "command".into(),
                    });
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    kind: event::KeyEventKind::Press,
                    ..
                }) => {
                    composer.pop();
                    if composer.starts_with('/') {
                        app.reset_slash_autocomplete();
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(ch),
                    modifiers,
                    kind: event::KeyEventKind::Press,
                    ..
                }) if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
                {
                    composer.push(ch);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: event::KeyEventKind::Press,
                    ..
                }) => {
                    let prompt = std::mem::take(&mut composer);
                    if prompt.is_empty() || cancel.is_some() {
                        continue;
                    }
                    if prompt.starts_with('/') {
                        match parse_command(&prompt) {
                            Ok(Command::Help) => app.start_request(
                                "command".into(),
                                "Commands: /help /about /clear /exit".into(),
                            ),
                            Ok(Command::About) => app.start_request(
                                "command".into(),
                                "LUMINUS — Illuminate the codebase. Offline fake provider.".into(),
                            ),
                            Ok(Command::Clear) => app.clear(),
                            Ok(Command::Exit) => should_exit = true,
                            Ok(Command::Model(role)) => {
                                let message = match model_catalog.select_role(role) {
                                    Ok(selection) => format!(
                                        "Model selected: {} / {}",
                                        selection.provider, selection.model
                                    ),
                                    Err(error) => error.to_string(),
                                };
                                app.start_request("command".into(), message);
                            }
                            Ok(Command::Provider(name)) => {
                                let text = match name {
                                    Some(name)
                                        if name == "openai" || name == "openai-compatible" =>
                                    {
                                        let cwd = std::env::current_dir()
                                            .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                        match OpenAiProvider::from_env(&cwd) {
                                            Some(Ok(openai)) => {
                                                let model = openai.model().id;
                                                provider = RuntimeProvider::OpenAi(openai);
                                                format!("Switched to provider: openai ({model})")
                                            }
                                            Some(Err(error)) => format!(
                                                "OpenAI config error: {error} (check .luminus/config.json or env)"
                                            ),
                                            None => {
                                                "OpenAI not configured: set api_key in .luminus/config.json or OPENAI_API_KEY".to_owned()
                                            }
                                        }
                                    }
                                    Some(name) if name == "fake" => {
                                        provider = RuntimeProvider::Fake(FakeProvider::new(
                                            Duration::from_millis(80),
                                        ));
                                        "Switched to provider: fake".to_owned()
                                    }
                                    Some(other) => format!("Unknown provider: {other}"),
                                    None => format!(
                                        "Current provider: {}\nAvailable: fake, openai (via env)",
                                        if provider.is_openai() {
                                            "openai"
                                        } else {
                                            "fake"
                                        }
                                    ),
                                };
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Discover) => {
                                let request_id = "command".to_owned();
                                let tx = provider_tx.clone();
                                let p = provider.clone();
                                tokio::spawn(async move {
                                    match p.list_models().await {
                                        Ok(models) => {
                                            let text = if models.is_empty() {
                                                "No models discovered.".to_owned()
                                            } else {
                                                format!(
                                                    "Discovered models:\\n{}",
                                                    models
                                                        .iter()
                                                        .map(|m| format!("  - {m}"))
                                                        .collect::<Vec<_>>()
                                                        .join("\\n")
                                                )
                                            };
                                            let _ = tx.send(ProviderEvent::Started {
                                                request_id: request_id.clone(),
                                            });
                                            let _ = tx.send(ProviderEvent::Delta {
                                                request_id: request_id.clone(),
                                                text,
                                            });
                                            let _ =
                                                tx.send(ProviderEvent::Completed { request_id });
                                        }
                                        Err(error) => {
                                            let _ = tx.send(ProviderEvent::Started {
                                                request_id: request_id.clone(),
                                            });
                                            let _ = tx.send(ProviderEvent::Failed {
                                                request_id,
                                                error: format!("Model discovery failed: {error}"),
                                            });
                                        }
                                    }
                                });
                            }
                            Ok(Command::Tools) => {
                                let text = tool_registry
                                    .specs()
                                    .iter()
                                    .map(|spec| {
                                        format!(
                                            "{} [{}] - {}",
                                            spec.name,
                                            spec.permission.label(),
                                            spec.description
                                        )
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                app.start_request(
                                    "command".into(),
                                    format!("Available tools:\n{text}"),
                                );
                            }
                            Ok(Command::Tool(name, args)) => {
                                let request = ToolRequest { name, args };
                                match tool_registry.prepare(request) {
                                    Ok(approval) => {
                                        apply_prepared_approval(
                                            &mut app,
                                            &tool_registry,
                                            approval,
                                            &mut cancel,
                                            &mut pending_tool,
                                        );
                                    }
                                    Err(error) => {
                                        app.start_request(
                                            "command".into(),
                                            format!("tool error: {error}"),
                                        );
                                        app.apply_provider_event(ProviderEvent::Completed {
                                            request_id: "command".into(),
                                        });
                                    }
                                }
                            }
                            Ok(Command::Save(name)) => {
                                let session = app.snapshot_session(name.clone());
                                let text = match session.save(default_root()) {
                                    Ok(path) => format!("Session saved: {}", path.display()),
                                    Err(error) => format!("Session save failed: {error}"),
                                };
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Sessions) => {
                                let text = match Session::list(default_root()) {
                                    Ok(names) if names.is_empty() => {
                                        "No saved sessions.".to_owned()
                                    }
                                    Ok(names) => format!(
                                        "Saved sessions:\n{}",
                                        names
                                            .iter()
                                            .map(|n| format!("  - {n}"))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    ),
                                    Err(error) => format!("Session listing failed: {error}"),
                                };
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Load(name)) => {
                                let text = match Session::load(default_root(), &name) {
                                    Ok(session) => {
                                        app.restore_session(&session);
                                        format!("Session loaded: {}", session.name)
                                    }
                                    Err(error) => format!("Session load failed: {error}"),
                                };
                                if !app.messages.iter().any(|message| message.content == text) {
                                    app.start_request("command".into(), text);
                                }
                            }
                            Ok(Command::Spawn(child_prompt)) => {
                                if app.active_agent_request_id().is_some() {
                                    app.start_request(
                                        "command".into(),
                                        "An agent is already running. Press Esc to cancel it first.".into(),
                                    );
                                } else {
                                    let request_id = Uuid::new_v4().to_string();
                                    let agent_id = luminus::agent::short_id(&request_id);
                                    let token = CancellationToken::new();
                                    if app.start_agent(
                                        agent_id.clone(),
                                        request_id.clone(),
                                        child_prompt.clone(),
                                    ) {
                                        agent_cancel = Some(token.clone());
                                        let tx = provider_tx.clone();
                                        let p = provider.clone();
                                        tokio::spawn(async move {
                                            for event in
                                                p.stream(request_id, child_prompt, token).await
                                            {
                                                let _ = tx.send(event);
                                            }
                                        });
                                    }
                                }
                            }
                            Ok(Command::Diff) => {
                                app.show_diff_view();
                            }
                            Ok(Command::Changes) => {
                                let text = app.handle_changes();
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Undo) => {
                                let text = app.handle_undo();
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Redo) => {
                                let text = app.handle_redo();
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::RevertFile(path)) => {
                                let text = app.handle_revert_file(&path);
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Skills) => {
                                let text = app.handle_skills_list();
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::SkillInspect(name)) => {
                                let text = app.handle_skill_inspect(&name);
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::SkillUse(name)) => {
                                let text = app.handle_skill_use(&name);
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::Env(key, val)) => {
                                let root = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let text = match luminus::setup::SetupWizard::save_env_var(
                                    &root, &key, &val,
                                ) {
                                    Ok(_) => {
                                        // SAFETY: we are the only thread mutating env at this point.
                                        unsafe {
                                            std::env::set_var(&key, &val);
                                        }
                                        format!(
                                            "Set {}={} (saved to .env and active in memory)",
                                            key, val
                                        )
                                    }
                                    Err(e) => format!("Failed to write .env: {}", e),
                                };
                                app.start_request("command".into(), text);
                            }
                            Ok(Command::McpList) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let config = luminus::mcp::config::McpConfig::load(&cwd);
                                app.start_request("command".into(), config.list_servers());
                            }
                            Ok(Command::McpConnect) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let config = luminus::mcp::config::McpConfig::load(&cwd);
                                let results = mcp_manager.connect_all(&config).await;
                                let mut lines = vec!["Connecting to MCP Servers...".to_owned()];
                                for (srv, res) in results {
                                    match res {
                                        Ok(()) => lines.push(format!("  {} -> connected ok", srv)),
                                        Err(e) => lines.push(format!("  {} -> error: {}", srv, e)),
                                    }
                                }
                                let specs = mcp_manager.dynamic_tool_specs();
                                if !specs.is_empty() {
                                    lines.push(format!("\nDiscovered {} MCP tools:", specs.len()));
                                    for s in specs {
                                        lines.push(format!("  {} ({})", s.name, s.description));
                                    }
                                }
                                app.start_request("command".into(), lines.join("\n"));
                            }
                            Ok(Command::Models) => {
                                use crate::command::help_text;
                                let mut lines = vec!["Configured models:".to_owned()];
                                for sel in model_catalog.list() {
                                    let active = if model_catalog.active() == Some(sel) {
                                        " (active)"
                                    } else {
                                        ""
                                    };
                                    lines.push(format!(
                                        "  {} -> {} / {}{}",
                                        sel.role, sel.provider, sel.model, active
                                    ));
                                }
                                lines.push(String::new());
                                lines.push(help_text());
                                app.start_request("command".into(), lines.join("\n"));
                            }
                            Ok(Command::Context) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let ctx = luminus::project_context::ProjectContext::discover(&cwd);
                                if ctx.loaded_paths.is_empty() {
                                    app.start_request(
                                        "command".into(),
                                        "No project context files found (LUMINUS.md, AGENTS.md, .luminus/instructions.md).".to_owned(),
                                    );
                                } else {
                                    let mut lines =
                                        vec!["Loaded project context files:".to_owned()];
                                    for p in &ctx.loaded_paths {
                                        lines.push(format!("  {}", p.display()));
                                    }
                                    lines.push(String::new());
                                    lines.push(ctx.formatted_instructions());
                                    app.start_request("command".into(), lines.join("\n"));
                                }
                            }
                            Ok(Command::Memory(arg)) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let store = luminus::self_improve::SelfImproveStore::new(&cwd);
                                match arg.as_deref() {
                                    None | Some("inspect") => {
                                        let soul = store.read_soul().unwrap_or_default();
                                        if soul.is_empty() {
                                            app.start_request(
                                                "command".into(),
                                                "Memory (soul.md) is empty.".to_owned(),
                                            );
                                        } else {
                                            app.start_request("command".into(), soul);
                                        }
                                    }
                                    Some(text) => match store.write_soul(text) {
                                        Ok(path) => {
                                            app.start_request(
                                                "command".into(),
                                                format!("Memory written to {}", path.display()),
                                            );
                                        }
                                        Err(e) => {
                                            app.start_request(
                                                "command".into(),
                                                format!("Error writing memory: {e}"),
                                            );
                                        }
                                    },
                                }
                            }
                            Ok(Command::Missions) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let store = luminus::mission::MissionStore::new(&cwd);
                                let missions = store.list();
                                if missions.is_empty() {
                                    app.start_request(
                                        "command".into(),
                                        "No missions found.".to_owned(),
                                    );
                                } else {
                                    let mut lines = vec!["Missions:".to_owned()];
                                    for m in missions {
                                        lines.push(format!(
                                            "  {} | {} | {:?}",
                                            m.id, m.title, m.status
                                        ));
                                    }
                                    app.start_request("command".into(), lines.join("\n"));
                                }
                            }
                            Ok(Command::Init) => {
                                let cwd = std::env::current_dir()
                                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                                let dir = cwd.join(".luminus");
                                let _ = std::fs::create_dir_all(&dir);
                                let path = dir.join("instructions.md");
                                let template = "# Luminus Project Instructions\n\nDescribe your project conventions, coding standards, and constraints here.\n";
                                match std::fs::write(&path, template) {
                                    Ok(()) => {
                                        app.start_request(
                                            "command".into(),
                                            format!("Created {}", path.display()),
                                        );
                                    }
                                    Err(e) => {
                                        app.start_request(
                                            "command".into(),
                                            format!("Error creating instructions: {e}"),
                                        );
                                    }
                                }
                            }
                            Err(error) => app.start_request("command".into(), error.to_string()),
                        }
                        if !should_exit && !matches!(prompt.as_str(), "/clear") {
                            app.apply_provider_event(ProviderEvent::Completed {
                                request_id: "command".into(),
                            });
                        }
                    } else {
                        let request_id = Uuid::new_v4().to_string();
                        let token = CancellationToken::new();
                        app.start_request(request_id.clone(), prompt.clone());
                        cancel = Some(token.clone());
                        let tx = provider_tx.clone();
                        let p = provider.clone();
                        tokio::spawn(async move {
                            for event in p.stream(request_id, prompt, token).await {
                                let _ = tx.send(event);
                            }
                        });
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

/// Background tool execution so the TUI can keep polling Esc/Ctrl+C cancel tokens.
struct PendingToolExec {
    id: ToolCallId,
    approval: ApprovalRequest,
    started: Instant,
    cancel: CancellationToken,
    rx: std_mpsc::Receiver<Result<ToolOutput, ToolError>>,
}

/// Start an approved tool on a worker thread with a cancellable token.
///
/// Shell commands honour the token via `execute_with_cancel`; other tools ignore it.
/// The event loop keeps drawing and can cancel with Esc / Ctrl+C.
fn start_approved_tool(
    app: &mut App,
    tool_registry: &ToolRegistry,
    approval: ApprovalRequest,
    cancel_slot: &mut Option<CancellationToken>,
    pending_tool: &mut Option<PendingToolExec>,
) {
    if pending_tool.is_some() {
        app.start_command(
            "command",
            "tool error: another tool is already running (Esc/Ctrl+C to cancel)",
        );
        app.apply_provider_event(ProviderEvent::Completed {
            request_id: "command".into(),
        });
        return;
    }

    let id = ToolCallId::new();
    app.begin_tool(id, approval.request.name.clone());
    let started = Instant::now();
    let token = CancellationToken::new();
    *cancel_slot = Some(token.clone());

    let (tx, rx) = std_mpsc::channel();
    let registry = *tool_registry;
    let approval_for_worker = approval.clone();
    let worker_token = token.clone();
    thread::spawn(move || {
        let result = registry.execute_with_cancel(&approval_for_worker, Some(&worker_token));
        let _ = tx.send(result);
    });

    *pending_tool = Some(PendingToolExec {
        id,
        approval,
        started,
        cancel: token,
        rx,
    });
}

fn finish_tool_execution(
    app: &mut App,
    pending: PendingToolExec,
    result: Result<ToolOutput, ToolError>,
) {
    let duration = pending.started.elapsed();
    let message = app.record_tool_result(pending.id, &pending.approval, result, duration);
    app.start_command("command", message);
    app.apply_provider_event(ProviderEvent::Completed {
        request_id: "command".into(),
    });
}

/// Reject a tool (once or session-deny) through the shared cancelled lifecycle.
fn reject_tool(app: &mut App, tool_name: impl Into<String>) {
    let message = app.record_tool_rejection(tool_name);
    app.start_command("command", message);
    app.apply_provider_event(ProviderEvent::Completed {
        request_id: "command".into(),
    });
}

/// Apply an operator choice from the approval overlay.
fn handle_approval_choice(
    app: &mut App,
    tool_registry: &ToolRegistry,
    choice: ApprovalChoice,
    cancel_slot: &mut Option<CancellationToken>,
    pending_tool: &mut Option<PendingToolExec>,
) {
    match choice {
        ApprovalChoice::AllowOnce | ApprovalChoice::AllowSession | ApprovalChoice::AllowProject => {
            if let Some(approval) = app.resolve_approval(choice) {
                start_approved_tool(app, tool_registry, approval, cancel_slot, pending_tool);
            }
            if let Some(error) = app.last_policy_error.take() {
                // Surface project-policy save failures without aborting the tool run.
                app.start_command("command", format!("policy: {error}"));
                app.apply_provider_event(ProviderEvent::Completed {
                    request_id: "command".into(),
                });
            }
        }
        ApprovalChoice::Reject
        | ApprovalChoice::RejectDenySession
        | ApprovalChoice::RejectDenyProject => {
            let tool_name = app
                .resolve_approval(choice)
                .map(|a| a.request.name)
                .unwrap_or_else(|| "tool".into());
            reject_tool(app, tool_name);
            if let Some(error) = app.last_policy_error.take() {
                app.start_command("command", format!("policy: {error}"));
                app.apply_provider_event(ProviderEvent::Completed {
                    request_id: "command".into(),
                });
            }
        }
    }
}

/// Route a prepared approval through session/project policy, then execute / deny / prompt.
fn apply_prepared_approval(
    app: &mut App,
    tool_registry: &ToolRegistry,
    approval: ApprovalRequest,
    cancel_slot: &mut Option<CancellationToken>,
    pending_tool: &mut Option<PendingToolExec>,
) {
    match app.gate_approval(approval) {
        ApprovalGate::SessionAllowed(approval) | ApprovalGate::ProjectAllowed(approval) => {
            start_approved_tool(app, tool_registry, approval, cancel_slot, pending_tool);
        }
        ApprovalGate::SessionDenied { tool } => {
            // Emit cancelled lifecycle card, then a clear session-deny transcript line.
            let _ = app.record_tool_rejection(tool.clone());
            app.start_command("command", format!("tool {tool}: denied for this session"));
            app.apply_provider_event(ProviderEvent::Completed {
                request_id: "command".into(),
            });
        }
        ApprovalGate::ProjectDenied { tool } => {
            let _ = app.record_tool_rejection(tool.clone());
            app.start_command("command", format!("tool {tool}: denied by project policy"));
            app.apply_provider_event(ProviderEvent::Completed {
                request_id: "command".into(),
            });
        }
        ApprovalGate::NeedsPrompt => {
            // Overlay is already active via gate_approval → show_approval.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn cli_accepts_empty_arguments() {
        assert!(Cli::try_parse_from(["luminus"]).is_ok());
    }
}
