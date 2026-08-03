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
    command::{Command, parse_command},
    event::ProviderEvent,
    model::{ModelCatalog, ModelRole, ModelSelection},
    provider::{FakeProvider, Provider},
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
    let (provider_tx, mut provider_rx) = mpsc::unbounded_channel::<ProviderEvent>();
    let provider = FakeProvider::new(Duration::from_millis(80));
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
            if event.is_terminal() {
                cancel = None;
            }
            app.apply_provider_event(event);
        }

        if event::poll(Duration::from_millis(40))? {
            match event::read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers,
                    ..
                }) if modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(token) = &cancel {
                        token.cancel();
                    } else {
                        should_exit = true;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                }) => {
                    if let Some(token) = &cancel {
                        token.cancel();
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    composer.pop();
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(ch),
                    modifiers,
                    ..
                }) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    composer.push(ch);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
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
                        let p = provider;
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
