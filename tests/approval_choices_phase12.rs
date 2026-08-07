//! Phase 12 granular approval choices (Hermes-like / Intruksi-style).
//!
//! Covers session allowlist auto-approve, denylist auto-deny, clear() reset,
//! and key-choice semantics via `App` methods (no full TUI event loop).

use luminus::{
    app::{App, ApprovalChoice, ApprovalGate, SessionToolPolicy, UiMode},
    tools::{ToolRegistry, ToolRequest},
};

fn prepare_read() -> luminus::tools::ApprovalRequest {
    ToolRegistry
        .prepare(ToolRequest {
            name: "read_file".into(),
            args: vec!["Cargo.toml".into()],
        })
        .expect("read_file should prepare")
}

fn prepare_list() -> luminus::tools::ApprovalRequest {
    ToolRegistry
        .prepare(ToolRequest {
            name: "list_dir".into(),
            args: vec![".".into()],
        })
        .expect("list_dir should prepare")
}

#[test]
fn allow_session_auto_approves_matching_tool_without_overlay() {
    let mut app = App::default();
    let first = prepare_read();
    app.show_approval(first);
    assert_eq!(app.ui_mode(), UiMode::Approval);

    let approved = app
        .resolve_approval(ApprovalChoice::AllowSession)
        .expect("AllowSession should return the pending request");
    assert_eq!(approved.request.name, "read_file");
    assert_eq!(app.ui_mode(), UiMode::Normal);
    assert!(app.pending_approval.is_none());
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );

    // Second matching tool: gate should auto-allow without overlay.
    let second = prepare_read();
    match app.gate_approval(second.clone()) {
        ApprovalGate::SessionAllowed(req) => {
            assert_eq!(req.request.name, "read_file");
            assert_eq!(app.ui_mode(), UiMode::Normal);
            assert!(app.pending_approval.is_none());
        }
        other => panic!("expected SessionAllowed, got {other:?}"),
    }

    // Different tool still needs a prompt.
    let other = prepare_list();
    match app.gate_approval(other) {
        ApprovalGate::NeedsPrompt => {
            assert_eq!(app.ui_mode(), UiMode::Approval);
            assert!(app.pending_approval.is_some());
        }
        other => panic!("expected NeedsPrompt for unmatched tool, got {other:?}"),
    }
}

#[test]
fn reject_deny_session_auto_denies_matching_tool_without_overlay() {
    let mut app = App::default();
    app.show_approval(prepare_read());

    let rejected = app
        .resolve_approval(ApprovalChoice::RejectDenySession)
        .expect("RejectDenySession should return the pending request");
    assert_eq!(rejected.request.name, "read_file");
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Denied)
    );

    let again = prepare_read();
    match app.gate_approval(again) {
        ApprovalGate::SessionDenied { tool } => {
            assert_eq!(tool, "read_file");
            assert_eq!(app.ui_mode(), UiMode::Normal);
            assert!(app.pending_approval.is_none());
        }
        other => panic!("expected SessionDenied, got {other:?}"),
    }
}

#[test]
fn clear_resets_session_allow_and_deny_lists() {
    let mut app = App::default();
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );

    app.show_approval(prepare_list());
    let _ = app.resolve_approval(ApprovalChoice::RejectDenySession);
    assert_eq!(
        app.session_policy("list_dir"),
        Some(SessionToolPolicy::Denied)
    );

    app.clear();

    assert_eq!(app.session_policy("read_file"), None);
    assert_eq!(app.session_policy("list_dir"), None);
    assert_eq!(app.ui_mode(), UiMode::Normal);
    assert!(app.pending_approval.is_none());
}

#[test]
fn key_semantics_allow_once_and_reject_once_do_not_persist_session_policy() {
    let mut app = App::default();
    app.show_approval(prepare_read());
    let _ = app
        .resolve_approval(ApprovalChoice::AllowOnce)
        .expect("AllowOnce");
    assert_eq!(app.session_policy("read_file"), None);

    // Next request still needs a prompt.
    match app.gate_approval(prepare_read()) {
        ApprovalGate::NeedsPrompt => {}
        other => panic!("AllowOnce must not create session policy: {other:?}"),
    }
    let _ = app.resolve_approval(ApprovalChoice::Reject);
    assert_eq!(app.session_policy("read_file"), None);

    match app.gate_approval(prepare_read()) {
        ApprovalGate::NeedsPrompt => {}
        other => panic!("Reject once must not create session policy: {other:?}"),
    }
}

#[test]
fn denylist_takes_precedence_over_allowlist() {
    let mut app = App::default();
    // Seed both lists via public choice path, then force allow after deny.
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );

    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::RejectDenySession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Denied),
        "deny must override a prior session allow for the same tool"
    );

    match app.gate_approval(prepare_read()) {
        ApprovalGate::SessionDenied { tool } => assert_eq!(tool, "read_file"),
        other => panic!("deny must win over allow: {other:?}"),
    }
}

#[test]
fn allow_session_after_deny_re_allows_tool() {
    let mut app = App::default();
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::RejectDenySession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Denied)
    );

    // User can still be prompted if we bypass gate (e.g. force show) — but
    // gate_approval will deny. Simulate re-prompt via show_approval directly
    // then AllowSession to flip policy.
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );

    match app.gate_approval(prepare_read()) {
        ApprovalGate::SessionAllowed(_) => {}
        other => panic!("allow after deny should re-enable: {other:?}"),
    }
}

#[test]
fn approval_choice_variants_are_distinct() {
    assert_ne!(ApprovalChoice::AllowOnce, ApprovalChoice::AllowSession);
    assert_ne!(ApprovalChoice::Reject, ApprovalChoice::RejectDenySession);
    assert_eq!(SessionToolPolicy::Allowed, SessionToolPolicy::Allowed);
    assert_eq!(SessionToolPolicy::Denied, SessionToolPolicy::Denied);
}
