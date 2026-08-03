//! Phase 6: responsive TUI header and footer rendering.
//!
//! These tests intentionally assert on semantic labels rather than coordinates or
//! padding, so they remain stable as the responsive layout evolves.

use luminus::{
    app::{App, Message, Role},
    tui::{Theme, render_to_string_with_composer},
};

fn app_with_session() -> App {
    let mut app = App::default();
    app.messages.push(Message {
        role: Role::User,
        content: "inspect the project".to_owned(),
    });
    app.messages.push(Message {
        role: Role::Assistant,
        content: "Ready to help.".to_owned(),
    });
    app
}

#[test]
fn wide_header_exposes_identity_and_command_help() {
    let output = render_to_string_with_composer(
        &app_with_session(),
        120,
        40,
        Theme::luminus(false),
        "status check",
    );

    assert!(
        output.contains("LUMINUS") || output.contains("CAPABILITIES") || output.contains("SESSION"),
        "wide header should expose product or session information:\n{output}"
    );
    assert!(
        output.contains("/help") || output.contains("Enter") || output.contains("Prompt"),
        "wide layout should expose command/help affordances:\n{output}"
    );
}

#[test]
fn medium_header_remains_usable_with_composer_and_help_affordance() {
    let output = render_to_string_with_composer(
        &app_with_session(),
        80,
        30,
        Theme::luminus(false),
        "medium prompt",
    );

    assert!(!output.trim().is_empty());
    assert!(
        output.contains("medium prompt"),
        "composer disappeared:\n{output}"
    );
    assert!(
        output.contains("STATUS") || output.contains("COMPOSER") || output.contains("Enter"),
        "medium layout lost its usable status/help affordance:\n{output}"
    );
}

#[test]
fn narrow_header_preserves_composer_and_status_without_panicking() {
    let output = render_to_string_with_composer(
        &app_with_session(),
        48,
        24,
        Theme::luminus(true),
        "narrow prompt",
    );

    assert!(
        output.contains("narrow prompt"),
        "composer disappeared:\n{output}"
    );
    assert!(
        output.contains("STATUS") || output.contains("COMPOSER"),
        "narrow layout lost status/composer labeling:\n{output}"
    );
}

#[test]
fn monochrome_render_contains_no_ansi_escape_sequences() {
    let output = render_to_string_with_composer(
        &app_with_session(),
        100,
        30,
        Theme::luminus(true),
        "plain terminal",
    );

    assert!(
        !output.contains('\u{1b}'),
        "monochrome string rendering must not contain ANSI escapes: {output:?}"
    );
}

#[test]
fn compact_width_keeps_rendering_a_nonempty_frame() {
    let output = render_to_string_with_composer(&App::default(), 32, 12, Theme::luminus(true), "x");
    assert!(!output.trim().is_empty());
}

#[test]
fn no_color_environment_does_not_change_plain_string_contract() {
    // `render_to_string_with_composer` uses Ratatui's TestBackend, whose output
    // is already plain cell text; this mirrors the NO_COLOR expectation without
    // mutating process-global environment state.
    let output =
        render_to_string_with_composer(&App::default(), 80, 24, Theme::luminus(true), "no color");
    assert!(!output.contains('\u{1b}'));
}

#[allow(dead_code)]
fn _role_is_reachable_for_future_header_fixtures() -> Role {
    Role::System
}
