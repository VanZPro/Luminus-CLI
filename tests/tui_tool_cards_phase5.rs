//! Phase 5: tool activity cards rendered in the TUI conversation area.
//!
//! Snapshot-style assertions against the public `render_to_string` /
//! `render_to_string_with_composer` APIs. Cards come from
//! `ToolActivity::card()`, which is line-oriented and stable, so we assert on
//! its exact text appearing in the rendered frame.

use std::time::Duration;

use luminus::{
    app::{App, Message, Role},
    tool_activity::ToolActivity,
    tui::{Theme, render_to_string, render_to_string_with_composer},
};

fn app_with_activities(activities: Vec<ToolActivity>) -> App {
    let mut app = App::default();
    app.messages.push(Message {
        role: Role::User,
        content: "run the linter".to_owned(),
    });
    app.messages.push(Message {
        role: Role::Assistant,
        content: "Running tools now.".to_owned(),
    });
    app.tool_activities = activities;
    app
}

#[test]
fn tool_cards_render_after_messages_in_wide_layout() {
    let app = app_with_activities(vec![
        ToolActivity::started("cargo_check"),
        ToolActivity::completed("cargo_check", "0 warnings"),
    ]);
    let output = render_to_string(&app, 120, 40, Theme::luminus(false));

    // Section header separates cards from messages.
    assert!(output.contains("TOOLS"), "missing TOOLS section:\n{output}");
    // Card header lines follow ToolActivity::card() formatting.
    assert!(output.contains("[started] cargo_check"));
    assert!(output.contains("[completed] cargo_check"));
    // Detail line from the completed card.
    assert!(output.contains("0 warnings"));

    // Cards appear after the conversation messages.
    let assistant_at = output
        .find("Running tools now.")
        .expect("assistant message should render");
    let card_at = output
        .find("[completed] cargo_check")
        .expect("card should render");
    assert!(
        card_at > assistant_at,
        "tool cards must render after messages"
    );
}

#[test]
fn failed_and_progress_cards_render_with_details() {
    let app = app_with_activities(vec![
        ToolActivity::progress("web_fetch", "downloading 42%"),
        ToolActivity::failed("web_fetch", "connection reset"),
    ]);
    let output = render_to_string(&app, 100, 32, Theme::luminus(false));

    assert!(output.contains("[progress] web_fetch"));
    assert!(output.contains("downloading 42%"));
    assert!(output.contains("[failed] web_fetch"));
    assert!(output.contains("connection reset"));
}

#[test]
fn duration_suffix_is_rendered() {
    let app = app_with_activities(vec![
        ToolActivity::completed("port_scan", "3 open ports")
            .with_duration(Duration::from_millis(1250)),
    ]);
    let output = render_to_string(&app, 100, 30, Theme::luminus(false));

    assert!(output.contains("[completed] port_scan (1250ms)"));
    assert!(output.contains("3 open ports"));
}

#[test]
fn narrow_monochrome_layout_stays_readable() {
    let app = app_with_activities(vec![
        ToolActivity::started("grep"),
        ToolActivity::completed("grep", "12 matches"),
    ]);
    // 48 columns triggers the compact layout; monochrome uses the fallback palette.
    let output = render_to_string(&app, 48, 24, Theme::luminus(true));

    assert!(output.contains("TOOLS"));
    assert!(output.contains("[started] grep"));
    assert!(output.contains("[completed] grep"));
    assert!(output.contains("12 matches"));
    // Conversation frame is still drawn around the cards.
    assert!(output.contains("CONVERSATION"));
}

#[test]
fn no_tools_section_without_activities() {
    let mut app = App::default();
    app.messages.push(Message {
        role: Role::User,
        content: "hello".to_owned(),
    });
    let output = render_to_string(&app, 100, 30, Theme::luminus(false));

    assert!(
        !output.contains("TOOLS"),
        "TOOLS should be hidden:\n{output}"
    );
}

#[test]
fn cards_render_alongside_composer_text() {
    let app = app_with_activities(vec![ToolActivity::completed("fmt", "clean")]);
    let output =
        render_to_string_with_composer(&app, 100, 30, Theme::luminus(false), "next prompt");

    assert!(output.contains("[completed] fmt"));
    assert!(output.contains("clean"));
    assert!(output.contains("next prompt"));
}

#[test]
fn long_card_detail_wraps_on_narrow_terminal_without_losing_prefix() {
    let detail = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let app = app_with_activities(vec![ToolActivity::completed("summarize", detail)]);
    let output = render_to_string(&app, 40, 30, Theme::luminus(true));

    // Header must survive the narrow width.
    assert!(output.contains("[completed] summarize"));
    // Wrapped detail keeps its words (across lines), so check a few tokens.
    for token in ["alpha", "epsilon", "kappa"] {
        assert!(output.contains(token), "missing {token}:\n{output}");
    }
}
