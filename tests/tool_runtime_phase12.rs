//! Phase 12 wiring: tool lifecycle events + bounded output on the approval path.
//!
//! These integration tests exercise `App` helpers the way `main.rs` does after
//! Y/Enter accept and N/Esc reject, without driving the full TUI event loop.

use luminus::{
    app::App,
    tool_activity::ToolActivity,
    tool_event::ToolCallId,
    tool_output::{BoundedOutput, Bounds},
    tools::{ToolRegistry, ToolRequest},
};
use std::time::Duration;

#[test]
fn approved_tool_execution_emits_lifecycle_and_bounded_transcript() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "read_file".into(),
            args: vec!["Cargo.toml".into()],
        })
        .expect("read_file approval should be preparable from crate root");

    let mut app = App::default();
    app.show_approval(approval.clone());
    let approval = app.take_approval().expect("pending approval");

    let id = ToolCallId::new();
    app.begin_tool(id, approval.request.name.clone());
    assert!(matches!(
        app.tool_activities.last(),
        Some(ToolActivity::Started { .. })
    ));

    let result = registry.execute(&approval);
    let summary = app.record_tool_result(id, &approval, result, Duration::from_millis(7));

    assert!(
        summary.contains(&format!("tool {id}")),
        "summary must carry the call id: {summary}"
    );
    assert!(
        summary.contains("read_file") && summary.contains("read-only"),
        "summary must name tool + permission: {summary}"
    );
    assert!(
        matches!(
            app.tool_activities.last(),
            Some(ToolActivity::Completed { meta, .. })
                if meta.duration == Some(Duration::from_millis(7))
        ),
        "completed activity should carry measured duration"
    );

    let transcript = app
        .messages
        .iter()
        .find(|m| m.content.contains(&format!("tool {id}")))
        .expect("bounded transcript message must be appended");
    assert!(
        transcript.content.contains("read_file") && transcript.content.contains("read-only"),
        "transcript should identify tool + permission"
    );
    // Cargo.toml is small — default bounds should not truncate.
    assert!(
        !transcript.content.contains("[truncated:"),
        "small Cargo.toml should not be truncated under default bounds"
    );

    // Command-log path must not wipe the lifecycle cards.
    app.start_command("command", summary);
    assert!(
        app.tool_activities.len() >= 2,
        "start_command must preserve tool activities"
    );
}

#[test]
fn large_tool_output_is_bounded_with_in_memory_full_output_note() {
    let mut app = App::default();
    let id = ToolCallId::new();
    // Force line truncation so metadata is deterministic without depending on
    // a real tool that produces huge output.
    let many_lines: String = (0..250).map(|i| format!("line-{i}\n")).collect();
    let bounds = Bounds {
        max_bytes: Some(4_096),
        max_lines: Some(200),
    };
    let bounded = BoundedOutput::truncate(&many_lines, bounds);
    assert!(
        bounded.truncated,
        "250 lines must exceed the 200-line default"
    );
    assert!(
        bounded.full_output.is_some(),
        "truncated outputs retain full payload in memory"
    );

    app.append_bounded_tool_output(id, "run_shell", "execute", &bounded);
    let content = &app.messages[0].content;
    assert!(content.contains(&format!("tool {id}")));
    assert!(content.contains("run_shell") && content.contains("execute"));
    assert!(content.contains("truncated"));
    assert!(content.contains(&format!("total_bytes={}", bounded.total_bytes)));
    assert!(content.contains(&format!("total_lines={}", bounded.total_lines)));
    assert!(content.contains(&format!("bytes_omitted={}", bounded.bytes_omitted)));
    assert!(content.contains(&format!("lines_omitted={}", bounded.lines_omitted)));
    assert!(content.contains("full_output available in-memory only"));
    assert!(content.contains("no disk artifact yet"));
}

#[test]
fn rejected_tool_emits_cancelled_lifecycle_with_id() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "list_dir".into(),
            args: vec![".".into()],
        })
        .expect("list_dir approval should be preparable");

    let mut app = App::default();
    app.show_approval(approval);
    let rejected = app
        .reject_approval()
        .expect("reject should return the pending request");
    let message = app.record_tool_rejection(rejected.request.name);

    assert!(
        message.contains("rejected") && message.contains("list_dir"),
        "rejection message must identify the tool: {message}"
    );
    assert!(
        message.contains("tool:"),
        "rejection message must include a ToolCallId display form: {message}"
    );
    assert!(matches!(
        app.tool_activities.last(),
        Some(ToolActivity::Failed { error, .. }) if error == "cancelled"
    ));

    app.start_command("command", message);
    assert_eq!(
        app.tool_activities.len(),
        1,
        "start_command must keep the cancelled activity card"
    );
}

#[test]
fn clear_still_clears_tool_activities_after_lifecycle_recording() {
    let mut app = App::default();
    let id = ToolCallId::new();
    app.begin_tool(id, "read_file");
    app.record_tool_rejection("write_file");
    assert!(!app.tool_activities.is_empty());

    app.clear();
    assert!(app.tool_activities.is_empty());
    assert!(app.messages.is_empty());
    assert!(app.pending_approval.is_none());
}
