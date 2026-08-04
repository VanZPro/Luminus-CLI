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
    app::App,
    command::{self, Command, parse_command},
    event::ProviderEvent,
    model::{ModelCatalog, ModelRole, ModelSelection},
    provider::{FakeProvider, Provider},
    providers::openai_runtime::{OpenAiProvider, RuntimeProvider},
    tui::{self, Theme},
};
use ratatui::{Terminal, backend::CrosstermBackend};
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
    let mut composer = String::new();
    let mut cancel: Option<CancellationToken> = None;
    let mut agent_cancel: Option<CancellationToken> = None;
    let (provider_tx, mut provider_rx) = mpsc::unbounded_channel::<ProviderEvent>();
    let mut provider = RuntimeProvider::from_env_or_fake(Duration::from_millis(80));
    let mut model_catalog = ModelCatalog::new();
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

        if event::poll(Duration::from_millis(40))? {
            let ev = event::read()?;

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
                    if let Some(token) = &cancel {
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
                    code: KeyCode::Backspace,
                    kind: event::KeyEventKind::Press,
                    ..
                }) => {
                    composer.pop();
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
                                    Some(name) if name == "openai" || name == "openai-compatible" => {
                                        match OpenAiProvider::from_env() {
                                            Some(Ok(openai)) => {
                                                let model = openai.model().id;
                                                provider = RuntimeProvider::OpenAi(openai);
                                                format!("Switched to provider: openai ({model})")
                                            }
                                            Some(Err(error)) => format!(
                                                "OpenAI config error: {error} (set OPENAI_API_KEY)"
                                            ),
                                            None => {
                                                "OpenAI not configured: set OPENAI_API_KEY (and optionally OPENAI_BASE_URL / OPENAI_MODEL)".to_owned()
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn cli_accepts_empty_arguments() {
        assert!(Cli::try_parse_from(["luminus"]).is_ok());
    }
}
