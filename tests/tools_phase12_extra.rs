//! Integration coverage for Phase 12 filesystem expansions
//! (`file_meta`/`file_metadata`, `glob`, `grep`, `edit_file`).

use luminus::tools::{
    Permission, PermissionDecision, RiskLevel, ToolError, ToolRegistry, ToolRequest,
};

#[test]
fn file_meta_prepare_and_execute() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "file_meta".into(),
            args: vec!["Cargo.toml".into()],
        })
        .expect("file_meta should prepare");
    assert_eq!(approval.spec.permission, Permission::ReadOnly);
    let metadata = approval.metadata().unwrap();
    assert_eq!(metadata.decision, PermissionDecision::Ask);
    assert_eq!(metadata.risk, RiskLevel::Low);
    assert_eq!(metadata.affected_paths.len(), 1);

    let out = registry.execute(&approval).unwrap().output;
    assert!(out.contains("is_file: true"));
    assert!(out.contains("path:"));
}

#[test]
fn glob_is_project_relative_only() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "glob".into(),
            args: vec!["src/*.rs".into()],
        })
        .expect("glob should prepare");
    let out = registry.execute(&approval).unwrap().output;
    assert!(
        out.lines().any(|l| l.contains("tools.rs")),
        "expected tools.rs in glob results: {out}"
    );

    assert!(matches!(
        registry.prepare(ToolRequest {
            name: "glob".into(),
            args: vec!["../../etc/*".into()],
        }),
        Err(ToolError::SecurityDenied(_))
    ));
}

#[test]
fn grep_defaults_to_project_root_and_formats_matches() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "grep".into(),
            args: vec!["ToolRegistry".into()],
        })
        .expect("grep should prepare with default path");
    let out = registry.execute(&approval).unwrap().output;
    assert!(
        out.contains(':') && out.lines().any(|l| l.contains("ToolRegistry")),
        "expected path:line:content style matches; got {out}"
    );
}

#[test]
fn edit_file_requires_three_args_and_denies_sensitive() {
    let registry = ToolRegistry;
    assert!(matches!(
        registry.prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec!["Cargo.toml".into(), "only-two".into()],
        }),
        Err(ToolError::MissingArgument(_))
    ));
    assert!(matches!(
        registry.prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec!["secrets.json".into(), "a".into(), "b".into()],
        }),
        Err(ToolError::SecurityDenied(_))
    ));
}
