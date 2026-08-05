use luminus::{
    app::{App, UiMode},
    command::{Command, parse_command},
    tools::{PermissionDecision, RiskLevel, ToolError, ToolRegistry, ToolRequest},
};

#[test]
fn clear_clears_pending_approval_and_resets_ui_mode() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "read_file".into(),
            args: vec!["Cargo.toml".into()],
        })
        .expect("read_file approval should be preparable");

    let mut app = App::default();
    app.show_approval(approval);
    assert_eq!(app.ui_mode(), UiMode::Approval);
    assert!(app.pending_approval.is_some());

    app.clear();

    assert_eq!(
        (app.ui_mode(), app.pending_approval.is_none()),
        (UiMode::Normal, true),
        "clear must leave no approval modal active and must discard stale pending approvals"
    );
}

#[test]
fn approval_metadata_includes_cwd_risk_affected_paths_and_reason() {
    let cwd = std::env::current_dir().expect("test should run from crate root");
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "read_file".into(),
            args: vec!["Cargo.toml".into()],
        })
        .expect("read_file approval should be preparable");

    let metadata = approval.metadata().expect("metadata should resolve");

    assert_eq!(metadata.decision, PermissionDecision::Ask);
    assert_eq!(metadata.risk, RiskLevel::Low);
    assert_eq!(metadata.cwd, cwd);
    assert_eq!(metadata.affected_paths.len(), 1);
    assert!(
        metadata.affected_paths[0].is_absolute(),
        "affected paths should be resolved for approval display"
    );
    assert!(
        metadata.affected_paths[0].starts_with(&metadata.cwd),
        "relative tool paths should resolve inside cwd"
    );
    assert!(
        metadata.affected_paths[0].ends_with("Cargo.toml"),
        "affected paths should identify the requested file"
    );
    assert!(
        metadata.reason.contains("read-only") && metadata.reason.contains("approval"),
        "reason should explain why approval is required; got {:?}",
        metadata.reason
    );
}

#[test]
fn sensitive_path_requests_are_denied_before_approval() {
    let registry = ToolRegistry;

    let result = registry.prepare(ToolRequest {
        name: "read_file".into(),
        args: vec![".env".into()],
    });

    assert!(
        matches!(result, Err(ToolError::SecurityDenied(ref reason)) if reason.contains("sensitive")),
        "sensitive credential-like paths must not produce approval prompts: {result:?}"
    );
}

#[test]
fn destructive_shell_requests_are_denied_before_approval() {
    let registry = ToolRegistry;

    let result = registry.prepare(ToolRequest {
        name: "run_shell".into(),
        args: vec!["git reset --hard HEAD".into()],
    });

    assert!(
        matches!(result, Err(ToolError::SecurityDenied(ref reason)) if reason.contains("destructive")),
        "destructive shell commands must not produce approval prompts: {result:?}"
    );
}

#[test]
fn tool_command_parser_documents_quoted_argument_limitation() {
    // Phase 12 still uses split_whitespace for /tool arguments. Quotes are not
    // interpreted as shell-like grouping, so callers cannot pass a single
    // argument containing spaces through the slash command parser yet.
    assert_eq!(
        parse_command("/tool write_file notes.txt \"hello world\""),
        Ok(Command::Tool(
            "write_file".into(),
            vec!["notes.txt".into(), "\"hello".into(), "world\"".into()]
        ))
    );
}
