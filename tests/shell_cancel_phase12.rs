//! Phase 12D: CancellationToken-aware cancellable shell execution.
//!
//! Covers cancel-before-start, cancel-during-run, fast success with a live
//! token, timeout without cancel, registry execute_with_cancel, and denylist.

use std::{
    thread,
    time::{Duration, Instant},
};

use luminus::tools::{
    ToolError, ToolRegistry, ToolRequest, run_shell_with_timeout,
    run_shell_with_timeout_cancellable,
};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
fn long_shell_cmd() -> &'static str {
    "ping -n 8 127.0.0.1 >nul"
}

#[cfg(not(windows))]
fn long_shell_cmd() -> &'static str {
    "sleep 5"
}

#[test]
fn cancel_before_start_returns_cancelled() {
    let token = CancellationToken::new();
    token.cancel();

    let err = run_shell_with_timeout_cancellable(
        "echo should-not-run",
        Duration::from_secs(5),
        Some(&token),
    )
    .expect_err("pre-cancelled token must short-circuit");

    match &err {
        ToolError::Cancelled(msg) => {
            assert!(
                msg.to_ascii_lowercase().contains("cancel"),
                "message should mention cancel; got {msg:?}"
            );
            let display = err.to_string();
            assert!(
                display.starts_with("cancelled:"),
                "Display must be 'cancelled: ...'; got {display:?}"
            );
        }
        other => panic!("expected ToolError::Cancelled, got {other}"),
    }
}

#[test]
fn cancel_during_long_command_returns_cancelled() {
    let token = CancellationToken::new();
    let cancel = token.clone();

    // Cancel shortly after the child starts so we exercise the mid-poll path.
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        cancel.cancel();
    });

    let started = Instant::now();
    let err =
        run_shell_with_timeout_cancellable(long_shell_cmd(), Duration::from_secs(30), Some(&token))
            .expect_err("long command must be cancelled");

    match err {
        ToolError::Cancelled(msg) => {
            assert!(
                msg.to_ascii_lowercase().contains("cancel"),
                "message should mention cancel; got {msg:?}"
            );
        }
        other => panic!("expected ToolError::Cancelled, got {other}"),
    }

    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(4),
        "child should be killed near cancel; took {elapsed:?}"
    );
}

#[test]
fn fast_command_succeeds_with_live_cancel_token() {
    let token = CancellationToken::new();
    let out = run_shell_with_timeout_cancellable(
        "echo phase12d-cancel-ok",
        Duration::from_secs(5),
        Some(&token),
    )
    .expect("fast echo should succeed under a live token");
    assert!(
        out.to_ascii_lowercase().contains("phase12d-cancel-ok"),
        "unexpected output: {out:?}"
    );
    assert!(!token.is_cancelled());
}

#[test]
fn timeout_still_works_without_cancel_token() {
    let started = Instant::now();
    let err = run_shell_with_timeout(long_shell_cmd(), Duration::from_secs(1))
        .expect_err("must time out without a cancel token");
    match err {
        ToolError::Timeout(msg) => {
            assert!(
                msg.contains("timeout") || msg.contains("exceeded"),
                "timeout message should mention timeout; got {msg:?}"
            );
        }
        other => panic!("expected ToolError::Timeout, got {other}"),
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(4),
        "child should be killed near the 1s deadline; took {elapsed:?}"
    );
}

#[test]
fn registry_execute_with_cancel_cancels_shell() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "run_shell".into(),
            args: vec![long_shell_cmd().into()],
        })
        .expect("non-destructive long shell should prepare");

    let token = CancellationToken::new();
    let cancel = token.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        cancel.cancel();
    });

    let started = Instant::now();
    let err = registry
        .execute_with_cancel(&approval, Some(&token))
        .expect_err("registry shell must honour cancel");

    match err {
        ToolError::Cancelled(_) => {}
        other => panic!("expected ToolError::Cancelled via registry, got {other}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "registry cancel path should kill promptly"
    );
}

#[test]
fn registry_execute_with_cancel_none_matches_execute() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "run_shell".into(),
            args: vec!["echo phase12d-registry-ok".into()],
        })
        .expect("non-destructive shell should prepare");

    let via_execute = registry.execute(&approval).expect("execute sync path");
    let via_cancel_none = registry
        .execute_with_cancel(&approval, None)
        .expect("execute_with_cancel(None) should match execute");

    assert_eq!(via_execute.tool, "run_shell");
    assert_eq!(via_execute.tool, via_cancel_none.tool);
    assert!(
        via_execute
            .output
            .to_ascii_lowercase()
            .contains("phase12d-registry-ok"),
        "unexpected output: {:?}",
        via_execute.output
    );
    assert_eq!(via_execute.output, via_cancel_none.output);
}

#[test]
fn destructive_denylist_unchanged_with_cancel_api() {
    let registry = ToolRegistry;
    let result = registry.prepare(ToolRequest {
        name: "run_shell".into(),
        args: vec!["rm -rf /".into()],
    });
    assert!(
        matches!(result, Err(ToolError::SecurityDenied(ref r)) if r.contains("destructive")),
        "destructive denylist must remain active with cancel API: {result:?}"
    );
}

#[test]
fn cancelled_display_prefix() {
    let err = ToolError::Cancelled("shell command cancelled".into());
    assert_eq!(err.to_string(), "cancelled: shell command cancelled");
}
