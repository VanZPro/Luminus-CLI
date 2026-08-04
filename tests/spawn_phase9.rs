//! Phase 9: spawned child-agent reducer and command parsing.
//!
//! Verifies the in-process pieces of `/spawn`: command parsing, the App
//! reducer routing events to the child agent without disturbing the main
//! request, single-active-agent policy, and TUI rendering of agent status.

use luminus::app::App;
use luminus::command::{Command, parse_command};
use luminus::event::ProviderEvent;
use luminus::tui::{Theme, render_to_string};

fn delta(request_id: &str, text: &str) -> ProviderEvent {
    ProviderEvent::Delta {
        request_id: request_id.into(),
        text: text.into(),
    }
}

#[test]
fn spawn_command_parses_non_empty_prompt() {
    assert_eq!(
        parse_command("/spawn review the architecture"),
        Ok(Command::Spawn("review the architecture".into()))
    );
    assert!(parse_command("/spawn").is_err());
    assert!(parse_command("/spawn   ").is_err());
}

#[test]
fn start_agent_records_running_state() {
    let mut app = App::default();
    assert!(app.start_agent("abc12345".into(), "req-1".into(), "review".into()));
    assert_eq!(app.agent_runs.len(), 1);
    assert_eq!(app.active_agent_request_id(), Some("req-1"));
}

#[test]
fn second_active_agent_is_rejected() {
    let mut app = App::default();
    assert!(app.start_agent("abc12345".into(), "req-1".into(), "p".into()));
    assert!(!app.start_agent("def67890".into(), "req-2".into(), "p".into()));
    assert_eq!(app.agent_runs.len(), 1);
}

#[test]
fn agent_deltas_do_not_touch_main_messages() {
    let mut app = App::default();
    app.start_request("main".into(), "hello".into());
    app.start_agent("abc12345".into(), "agent-1".into(), "child".into());

    assert!(app.apply_agent_event(&delta("agent-1", "child says hi")));
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].content, "hello");
    assert_eq!(app.agent_runs[0].output, "child says hi");
}

#[test]
fn main_request_events_do_not_touch_agent() {
    let mut app = App::default();
    app.start_request("main".into(), "hello".into());
    app.start_agent("abc12345".into(), "agent-1".into(), "child".into());

    assert!(!app.apply_agent_event(&delta("main", "main delta")));
    app.apply_provider_event(delta("main", "main delta"));
    assert_eq!(app.messages[1].content, "main delta");
    assert!(app.agent_runs[0].output.is_empty());
}

#[test]
fn completed_agent_becomes_terminal_and_late_deltas_ignored() {
    let mut app = App::default();
    app.start_agent("abc12345".into(), "agent-1".into(), "child".into());
    app.apply_agent_event(&delta("agent-1", "done"));
    app.apply_agent_event(&ProviderEvent::Completed {
        request_id: "agent-1".into(),
    });
    app.apply_agent_event(&delta("agent-1", "late"));

    assert_eq!(app.agent_runs[0].output, "done");
    assert!(app.agent_runs[0].status.is_terminal());
    assert_eq!(app.active_agent_request_id(), None);
}

#[test]
fn clear_resets_agent_runs() {
    let mut app = App::default();
    app.start_agent("abc12345".into(), "agent-1".into(), "child".into());
    app.clear();
    assert!(app.agent_runs.is_empty());
    assert!(app.active_agent_request_id().is_none());
}

#[test]
fn agent_status_renders_in_tui() {
    let mut app = App::default();
    app.start_agent("abc12345".into(), "agent-1".into(), "review code".into());
    app.apply_agent_event(&ProviderEvent::Completed {
        request_id: "agent-1".into(),
    });

    let text = render_to_string(&app, 80, 24, Theme::luminus(false));
    assert!(text.contains("AGENTS"));
    assert!(text.contains("agent-abc12345"));
}
