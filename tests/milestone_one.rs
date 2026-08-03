use std::time::Duration;

use luminus::{
    app::{App, Role},
    command::{Command, parse_command},
    event::ProviderEvent,
    provider::{FakeProvider, Provider},
    tui::{render_to_string, theme::Theme},
};
use tokio_util::sync::CancellationToken;

#[test]
fn parses_only_the_supported_slash_commands() {
    assert_eq!(parse_command("/help"), Ok(Command::Help));
    assert_eq!(parse_command("/about"), Ok(Command::About));
    assert_eq!(parse_command("/clear"), Ok(Command::Clear));
    assert_eq!(parse_command("/exit"), Ok(Command::Exit));
    assert!(parse_command("/model").is_err());
}

#[tokio::test]
async fn fake_provider_streams_one_terminal_outcome() {
    let provider = FakeProvider::new(Duration::ZERO);
    let events = provider
        .stream("request-1".into(), "hello".into(), CancellationToken::new())
        .await;

    assert!(matches!(
        events.first(),
        Some(ProviderEvent::Started { .. })
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Delta { .. }))
    );
    assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Completed { .. })
    ));
}

#[tokio::test]
async fn fake_provider_stops_after_cancellation() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let events = FakeProvider::new(Duration::ZERO)
        .stream("request-2".into(), "hello".into(), cancellation)
        .await;

    assert!(matches!(
        events.last(),
        Some(ProviderEvent::Cancelled { .. })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Delta { .. }))
    );
    assert_eq!(events.iter().filter(|event| event.is_terminal()).count(), 1);
}

#[test]
fn reducer_ignores_chunks_after_terminal_event() {
    let mut app = App::default();
    app.start_request("request-3".into(), "hello".into());
    app.apply_provider_event(ProviderEvent::Cancelled {
        request_id: "request-3".into(),
    });
    app.apply_provider_event(ProviderEvent::Delta {
        request_id: "request-3".into(),
        text: "late".into(),
    });

    assert!(
        !app.messages
            .iter()
            .any(|message| message.content.contains("late"))
    );
    assert!(
        app.messages
            .iter()
            .any(|message| message.role == Role::System)
    );
}

#[test]
fn render_is_responsive_and_monochrome_capable() {
    let app = App::default();
    let wide = render_to_string(&app, 120, 40, Theme::luminus(false));
    let narrow = render_to_string(&app, 60, 24, Theme::luminus(false));
    let mono = render_to_string(&app, 80, 24, Theme::luminus(true));

    assert!(wide.contains("LUMINUS"));
    assert!(wide.contains("Illuminate the codebase."));
    assert!(narrow.contains("◆ LUMINUS"));
    assert!(mono.contains("LUMINUS"));
}
