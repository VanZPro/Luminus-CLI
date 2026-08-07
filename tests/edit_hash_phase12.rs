//! Phase 12E: content-hash safe `edit_file` + unified diff preview.
//!
//! Covers unique replace, non-unique/missing failure, stale-hash reject,
//! matching-hash apply, CRLF preservation, and tool-output hash/diff markers.

use std::fs;
use std::path::PathBuf;

use luminus::tools::{ToolError, ToolRegistry, ToolRequest, content_hash_of, edit_file_with_hash};

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "luminus-edit-hash-{}-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        tag
    ))
}

#[test]
fn unique_replace_reports_hashes_and_unified_diff() {
    let path = temp_path("unique");
    fs::write(&path, "alpha beta gamma\n").unwrap();
    let path_s = path.to_string_lossy().into_owned();

    let registry = ToolRegistry;
    let approval = registry
        .prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec![path_s, "beta".into(), "BETA".into()],
        })
        .expect("prepare");
    let out = registry.execute(&approval).unwrap().output;

    assert!(
        out.contains("before_hash: dh64:"),
        "missing before_hash: {out}"
    );
    assert!(
        out.contains("after_hash: dh64:"),
        "missing after_hash: {out}"
    );
    assert!(out.contains("--- a/"), "missing --- marker: {out}");
    assert!(out.contains("+++ b/"), "missing +++ marker: {out}");
    assert!(out.contains("@@"), "missing @@ hunk: {out}");
    assert!(
        out.contains("-beta") && out.contains("+BETA"),
        "hunk body: {out}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "alpha BETA gamma\n");

    let _ = fs::remove_file(&path);
}

#[test]
fn non_unique_replace_fails() {
    let path = temp_path("ambiguous");
    fs::write(&path, "x y x\n").unwrap();

    let err = edit_file_with_hash(&path, "x", "X", None).unwrap_err();
    assert!(
        matches!(err, ToolError::EditFailed(ref r) if r.contains("ambiguous")),
        "got {err}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "x y x\n");
    let _ = fs::remove_file(&path);
}

#[test]
fn missing_old_string_fails() {
    let path = temp_path("missing");
    fs::write(&path, "hello\n").unwrap();

    let err = edit_file_with_hash(&path, "nope", "x", None).unwrap_err();
    assert!(
        matches!(err, ToolError::EditFailed(ref r) if r.contains("not found")),
        "got {err}"
    );
    let _ = fs::remove_file(&path);
}

#[test]
fn stale_hash_is_rejected() {
    let path = temp_path("stale");
    let body = b"one two three\n";
    fs::write(&path, body).unwrap();
    let real = content_hash_of(body);
    let stale = "dh64:0000000000000000";
    assert_ne!(real, stale);

    let err = edit_file_with_hash(&path, "two", "TWO", Some(stale)).unwrap_err();
    match err {
        ToolError::EditFailed(msg) => {
            assert!(
                msg.contains("stale content hash"),
                "expected stale message, got {msg}"
            );
            assert!(msg.contains(stale), "should mention expected: {msg}");
            assert!(msg.contains(&real), "should mention got: {msg}");
        }
        other => panic!("expected EditFailed, got {other}"),
    }
    assert_eq!(fs::read(&path).unwrap(), body);
    let _ = fs::remove_file(&path);
}

#[test]
fn matching_hash_applies() {
    let path = temp_path("match");
    let body = b"one two three\n";
    fs::write(&path, body).unwrap();
    let hash = content_hash_of(body);

    let out = edit_file_with_hash(&path, "two", "TWO", Some(&hash)).unwrap();
    assert!(out.contains(&format!("before_hash: {hash}")));
    assert!(out.contains("after_hash: dh64:"));
    assert!(out.contains("--- a/") && out.contains("+++ b/"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "one TWO three\n");

    // Hash of the new content should match after_hash line.
    let after = content_hash_of(b"one TWO three\n");
    assert!(out.contains(&format!("after_hash: {after}")));
    let _ = fs::remove_file(&path);
}

#[test]
fn crlf_line_endings_are_preserved() {
    let path = temp_path("crlf");
    // CRLF body with a unique token to replace.
    let body = "line1\r\nline TWO here\r\nline3\r\n";
    fs::write(&path, body.as_bytes()).unwrap();
    let hash = content_hash_of(body.as_bytes());

    let out = edit_file_with_hash(&path, "TWO", "2", Some(&hash)).unwrap();
    assert!(
        out.contains("line_endings: CRLF (preserved)"),
        "expected CRLF note: {out}"
    );

    let written = fs::read(&path).unwrap();
    // Must still be CRLF throughout (no bare LF introduced).
    assert!(
        written.windows(2).any(|w| w == b"\r\n"),
        "expected CRLF in written bytes"
    );
    assert!(
        !String::from_utf8_lossy(&written)
            .replace("\r\n", "")
            .contains('\n'),
        "bare LF leaked into CRLF file: {:?}",
        String::from_utf8_lossy(&written)
    );
    assert_eq!(written.as_slice(), b"line1\r\nline 2 here\r\nline3\r\n");
    let _ = fs::remove_file(&path);
}

#[test]
fn crlf_normalized_into_replacement_text() {
    let path = temp_path("crlf-new");
    let body = "keep\r\nold block\r\nend\r\n";
    fs::write(&path, body.as_bytes()).unwrap();

    // Caller passes LF-only multi-line new_string; file is CRLF.
    let out = edit_file_with_hash(&path, "old block", "new\nblock", None).unwrap();
    assert!(out.contains("CRLF (preserved)"));
    let written = fs::read(&path).unwrap();
    assert_eq!(written.as_slice(), b"keep\r\nnew\r\nblock\r\nend\r\n");
    let _ = fs::remove_file(&path);
}

#[test]
fn registry_accepts_optional_fourth_hash_arg() {
    let path = temp_path("registry-hash");
    let body = b"foo bar baz\n";
    fs::write(&path, body).unwrap();
    let hash = content_hash_of(body);
    let path_s = path.to_string_lossy().into_owned();

    let registry = ToolRegistry;

    // Stale via registry args[3].
    let stale = registry
        .prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec![
                path_s.clone(),
                "bar".into(),
                "BAR".into(),
                "dh64:deadbeefdeadbeef".into(),
            ],
        })
        .unwrap();
    let err = registry.execute(&stale).unwrap_err();
    assert!(
        matches!(err, ToolError::EditFailed(ref r) if r.contains("stale content hash")),
        "got {err}"
    );

    // Matching hash via registry.
    let ok = registry
        .prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec![path_s, "bar".into(), "BAR".into(), hash],
        })
        .unwrap();
    let out = registry.execute(&ok).unwrap().output;
    assert!(out.contains("before_hash:"));
    assert!(out.contains("--- a/"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "foo BAR baz\n");
    let _ = fs::remove_file(&path);
}

#[test]
fn content_hash_of_is_stable_and_prefixed() {
    let h1 = content_hash_of(b"hello");
    let h2 = content_hash_of(b"hello");
    let h3 = content_hash_of(b"world");
    assert!(h1.starts_with("dh64:"));
    assert_eq!(h1.len(), "dh64:".len() + 16);
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}
