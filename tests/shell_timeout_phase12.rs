//! Phase 12 shell timeout foundation: fast success + timeout kill.
//!
//! Full async CancellationToken cancel is Phase 12D; this slice covers
//! wall-clock timeout via `run_shell_with_timeout` / `ToolError::Timeout`.

use std::time::{Duration, Instant};

use luminus::tools::{
    DEFAULT_SHELL_TIMEOUT_SECS, ToolError, ToolRegistry, ToolRequest, run_shell_with_timeout,
    shell_timeout,
};

#[test]
fn shell_timeout_default_is_thirty_seconds_without_env() {
    // Only assert the constant contract; env may be polluted by other tests.
    assert_eq!(DEFAULT_SHELL_TIMEOUT_SECS, 30);
    let t = shell_timeout();
    assert!(
        t.as_secs() >= 1,
        "shell timeout must be at least 1s; got {t:?}"
    );
}

#[test]
fn fast_shell_command_succeeds_under_timeout() {
    let out = run_shell_with_timeout("echo phase12-shell-ok", Duration::from_secs(5))
        .expect("echo should finish well under timeout");
    assert!(
        out.to_ascii_lowercase().contains("phase12-shell-ok"),
        "unexpected output: {out:?}"
    );
}

#[test]
fn long_shell_command_fails_with_timeout_error() {
    let started = Instant::now();
    #[cfg(windows)]
    let cmd = "ping -n 8 127.0.0.1 >nul";
    #[cfg(not(windows))]
    let cmd = "sleep 5";

    let err = run_shell_with_timeout(cmd, Duration::from_secs(1)).expect_err("must time out");
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
fn registry_run_shell_still_honours_destructive_denylist() {
    let registry = ToolRegistry;
    let result = registry.prepare(ToolRequest {
        name: "run_shell".into(),
        args: vec!["rm -rf /".into()],
    });
    assert!(
        matches!(result, Err(ToolError::SecurityDenied(ref r)) if r.contains("destructive")),
        "destructive denylist must remain active with timeout foundation: {result:?}"
    );
}

#[test]
fn registry_execute_fast_shell_via_approval_path() {
    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "run_shell".into(),
            args: vec!["echo luminus-registry-shell".into()],
        })
        .expect("non-destructive shell should prepare");
    let output = registry
        .execute(&approval)
        .expect("fast approved shell should execute");
    assert!(
        output
            .output
            .to_ascii_lowercase()
            .contains("luminus-registry-shell"),
        "unexpected tool output: {:?}",
        output.output
    );
}
