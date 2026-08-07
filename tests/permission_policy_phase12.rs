//! Phase 12D: project-persisted tool permission rules + gate ordering.
//!
//! Covers:
//! - load/save round-trip of `.luminus/tool_policy.json`
//! - gate precedence: session deny > project deny > session allow > project allow > prompt
//! - AllowProject / RejectDenyProject persist to disk
//! - `App::clear` resets session policies only (project rules survive)

use std::fs;

use luminus::{
    app::{App, ApprovalChoice, ApprovalGate, SessionToolPolicy, UiMode},
    permission_policy::{ProjectToolPolicy, ToolPolicy},
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

fn scratch_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "luminus-p12d-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

#[test]
fn project_policy_load_save_roundtrip() {
    let root = scratch_root("roundtrip");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut store = ProjectToolPolicy::empty(&root);
    store.set("read_file", ToolPolicy::Allowed);
    store.set("run_shell", ToolPolicy::Denied);
    let path = store.save().unwrap();
    assert!(path.is_file());
    assert!(path.file_name().and_then(|n| n.to_str()) == Some("tool_policy.json"));

    let loaded = ProjectToolPolicy::load(&root).unwrap();
    assert_eq!(loaded.get("read_file"), Some(ToolPolicy::Allowed));
    assert_eq!(loaded.get("run_shell"), Some(ToolPolicy::Denied));
    assert_eq!(loaded.get("list_dir"), None);

    // JSON shape is human-readable.
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains("read_file"));
    assert!(raw.contains("allowed") || raw.contains("Allowed") || raw.contains("\"allowed\""));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn load_missing_policy_is_empty_not_error() {
    let root = scratch_root("missing");
    let _ = fs::remove_dir_all(&root);
    let loaded = ProjectToolPolicy::load(&root).unwrap();
    assert!(loaded.is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_load_project_policy_and_gate_project_allow() {
    let root = scratch_root("gate-allow");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut store = ProjectToolPolicy::empty(&root);
    store.set("read_file", ToolPolicy::Allowed);
    store.save().unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();
    assert_eq!(app.project_policy("read_file"), Some(ToolPolicy::Allowed));

    match app.gate_approval(prepare_read()) {
        ApprovalGate::ProjectAllowed(req) => {
            assert_eq!(req.request.name, "read_file");
            assert_eq!(app.ui_mode(), UiMode::Normal);
            assert!(app.pending_approval.is_none());
        }
        other => panic!("expected ProjectAllowed, got {other:?}"),
    }

    // Unlisted tool still prompts.
    match app.gate_approval(prepare_list()) {
        ApprovalGate::NeedsPrompt => {
            assert_eq!(app.ui_mode(), UiMode::Approval);
        }
        other => panic!("expected NeedsPrompt, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn app_gate_project_deny() {
    let root = scratch_root("gate-deny");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut store = ProjectToolPolicy::empty(&root);
    store.set("read_file", ToolPolicy::Denied);
    store.save().unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();

    match app.gate_approval(prepare_read()) {
        ApprovalGate::ProjectDenied { tool } => {
            assert_eq!(tool, "read_file");
            assert_eq!(app.ui_mode(), UiMode::Normal);
        }
        other => panic!("expected ProjectDenied, got {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn gate_ordering_session_deny_beats_project_allow() {
    let root = scratch_root("order-session-deny");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut store = ProjectToolPolicy::empty(&root);
    store.set("read_file", ToolPolicy::Allowed);
    store.save().unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();

    // Force session deny via choice path.
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::RejectDenySession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Denied)
    );
    assert_eq!(app.project_policy("read_file"), Some(ToolPolicy::Allowed));

    match app.gate_approval(prepare_read()) {
        ApprovalGate::SessionDenied { tool } => assert_eq!(tool, "read_file"),
        other => panic!("session deny must beat project allow: {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn gate_ordering_project_deny_beats_session_allow() {
    let root = scratch_root("order-project-deny");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut store = ProjectToolPolicy::empty(&root);
    store.set("read_file", ToolPolicy::Denied);
    store.save().unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();

    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);
    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );
    assert_eq!(app.project_policy("read_file"), Some(ToolPolicy::Denied));

    match app.gate_approval(prepare_read()) {
        ApprovalGate::ProjectDenied { tool } => assert_eq!(tool, "read_file"),
        other => panic!("project deny must beat session allow: {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn gate_ordering_session_allow_beats_project_none() {
    // No project entry; session allow auto-approves.
    let mut app = App::default();
    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);

    match app.gate_approval(prepare_read()) {
        ApprovalGate::SessionAllowed(_) => {}
        other => panic!("expected SessionAllowed, got {other:?}"),
    }
}

#[test]
fn allow_project_persists_to_disk_and_auto_allows() {
    let root = scratch_root("allow-project");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();
    app.show_approval(prepare_read());
    let approved = app
        .resolve_approval(ApprovalChoice::AllowProject)
        .expect("AllowProject returns pending request");
    assert_eq!(approved.request.name, "read_file");
    assert_eq!(app.project_policy("read_file"), Some(ToolPolicy::Allowed));
    assert!(app.last_policy_error.is_none());

    // Disk has the entry.
    let reloaded = ProjectToolPolicy::load(&root).unwrap();
    assert_eq!(reloaded.get("read_file"), Some(ToolPolicy::Allowed));

    // Next gate auto-allows via project.
    match app.gate_approval(prepare_read()) {
        ApprovalGate::ProjectAllowed(_) => {}
        other => panic!("expected ProjectAllowed after AllowProject: {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn reject_deny_project_persists_to_disk_and_auto_denies() {
    let root = scratch_root("deny-project");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();
    app.show_approval(prepare_read());
    let rejected = app
        .resolve_approval(ApprovalChoice::RejectDenyProject)
        .expect("RejectDenyProject returns pending request");
    assert_eq!(rejected.request.name, "read_file");
    assert_eq!(app.project_policy("read_file"), Some(ToolPolicy::Denied));

    let reloaded = ProjectToolPolicy::load(&root).unwrap();
    assert_eq!(reloaded.get("read_file"), Some(ToolPolicy::Denied));

    match app.gate_approval(prepare_read()) {
        ApprovalGate::ProjectDenied { tool } => assert_eq!(tool, "read_file"),
        other => panic!("expected ProjectDenied after RejectDenyProject: {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_resets_session_policy_only_not_project() {
    let root = scratch_root("clear");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let mut app = App::default();
    app.load_project_policy(&root).unwrap();

    app.show_approval(prepare_read());
    let _ = app.resolve_approval(ApprovalChoice::AllowSession);
    app.show_approval(prepare_list());
    let _ = app.resolve_approval(ApprovalChoice::AllowProject);

    assert_eq!(
        app.session_policy("read_file"),
        Some(SessionToolPolicy::Allowed)
    );
    assert_eq!(app.project_policy("list_dir"), Some(ToolPolicy::Allowed));

    app.clear();

    assert_eq!(app.session_policy("read_file"), None);
    // Project rule survives clear (in-memory + disk).
    assert_eq!(app.project_policy("list_dir"), Some(ToolPolicy::Allowed));
    let reloaded = ProjectToolPolicy::load(&root).unwrap();
    assert_eq!(reloaded.get("list_dir"), Some(ToolPolicy::Allowed));

    // Gate still honours project allow after clear.
    match app.gate_approval(prepare_list()) {
        ApprovalGate::ProjectAllowed(_) => {}
        other => panic!("project allow must survive clear: {other:?}"),
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn approval_choice_project_variants_are_distinct() {
    assert_ne!(ApprovalChoice::AllowProject, ApprovalChoice::AllowSession);
    assert_ne!(
        ApprovalChoice::RejectDenyProject,
        ApprovalChoice::RejectDenySession
    );
    assert_ne!(ApprovalChoice::AllowProject, ApprovalChoice::AllowOnce);
    assert_eq!(ToolPolicy::Allowed, ToolPolicy::Allowed);
    assert_eq!(ToolPolicy::Denied, ToolPolicy::Denied);
}
