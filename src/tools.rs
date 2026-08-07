//! Permission-gated coding tools.
//!
//! Tools are deliberately small and synchronous in this phase. The registry
//! validates names/arguments and returns an approval request; callers must
//! explicitly approve before invoking a tool.

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::Hasher,
    io::{self, Read},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use tokio_util::sync::CancellationToken;

/// Soft caps for search tools so a single call cannot flood the transcript.
const GLOB_RESULT_CAP: usize = 200;
const GREP_MATCH_CAP: usize = 200;
/// Skip files larger than this during grep (binary dumps / minified noise).
const GREP_MAX_FILE_BYTES: u64 = 1_048_576;

/// Default `run_shell` wall-clock timeout when `LUMINUS_SHELL_TIMEOUT_SECS` is unset.
pub const DEFAULT_SHELL_TIMEOUT_SECS: u64 = 30;

/// Poll interval while waiting for a shell child to exit.
const SHELL_WAIT_POLL: Duration = Duration::from_millis(25);

/// The outcome of policy evaluation before a caller approves execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Ask,
    Deny,
}

/// Coarse risk tier used by callers to decide how to surface approval prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    ReadOnly,
    Write,
    Execute,
    Network,
}

impl Permission {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Write => "write",
            Self::Execute => "execute",
            Self::Network => "network",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub permission: Permission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRequest {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub request: ToolRequest,
    pub spec: ToolSpec,
}

/// Resolved approval metadata for UI prompts and policy decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalMetadata {
    pub decision: PermissionDecision,
    pub risk: RiskLevel,
    pub cwd: PathBuf,
    pub affected_paths: Vec<PathBuf>,
    pub reason: String,
}

impl ApprovalRequest {
    pub fn metadata(&self) -> Result<ApprovalMetadata, ToolError> {
        metadata_for(&self.spec, &self.request.args)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub tool: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    UnknownTool(String),
    MissingArgument(&'static str),
    PermissionDenied(Permission),
    Io(String),
    Process(String),
    NetworkDisabled,
    SecurityDenied(String),
    /// Edit failed because `old_string` was missing or not unique.
    EditFailed(String),
    /// Shell (or other bounded) tool exceeded its wall-clock timeout and was killed.
    Timeout(String),
    /// Shell (or other bounded) tool was cancelled via [`CancellationToken`].
    Cancelled(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTool(name) => write!(f, "unknown tool: {name}"),
            Self::MissingArgument(arg) => write!(f, "missing argument: {arg}"),
            Self::PermissionDenied(permission) => {
                write!(f, "permission denied: {}", permission.label())
            }
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Process(error) => write!(f, "process error: {error}"),
            Self::NetworkDisabled => f.write_str("network tools are disabled in this phase"),
            Self::SecurityDenied(reason) => {
                write!(f, "security policy denied request: {reason}")
            }
            Self::EditFailed(reason) => write!(f, "edit failed: {reason}"),
            Self::Timeout(reason) => write!(f, "timeout: {reason}"),
            Self::Cancelled(reason) => write!(f, "cancelled: {reason}"),
        }
    }
}

impl std::error::Error for ToolError {}

pub const TOOL_SPECS: [ToolSpec; 10] = [
    ToolSpec {
        name: "read_file",
        description: "read a UTF-8 text file",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "write_file",
        description: "write UTF-8 text to a file",
        permission: Permission::Write,
    },
    ToolSpec {
        name: "list_dir",
        description: "list directory entries",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "file_meta",
        description: "read basic file or directory metadata",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "file_metadata",
        description: "alias of file_meta",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "glob",
        description: "find paths matching a glob pattern under the project root",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "grep",
        description: "search file contents for a pattern (path:line:content)",
        permission: Permission::ReadOnly,
    },
    ToolSpec {
        name: "edit_file",
        description: "replace an exact unique string in a file; optional 4th arg expected_hash (dh64:…) rejects stale content; reports before/after hash + unified diff",
        permission: Permission::Write,
    },
    ToolSpec {
        name: "run_shell",
        description: "run one shell command",
        permission: Permission::Execute,
    },
    ToolSpec {
        name: "http_get",
        description: "fetch a URL (disabled by default)",
        permission: Permission::Network,
    },
];

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolRegistry;

impl ToolRegistry {
    pub fn specs(&self) -> &'static [ToolSpec] {
        &TOOL_SPECS
    }

    /// Validate arguments, canonicalize affected paths, classify risk, and
    /// produce a [`PermissionDecision`]. Deny decisions short-circuit as a
    /// [`ToolError::SecurityDenied`] so callers never see an approval prompt
    /// for blocked actions.
    pub fn prepare(&self, request: ToolRequest) -> Result<ApprovalRequest, ToolError> {
        let spec = TOOL_SPECS
            .iter()
            .find(|spec| spec.name == request.name)
            .cloned()
            .ok_or_else(|| ToolError::UnknownTool(request.name.clone()))?;
        validate_args(&spec, &request.args)?;

        let metadata = metadata_for(&spec, &request.args)?;
        if metadata.decision == PermissionDecision::Deny {
            return Err(ToolError::SecurityDenied(metadata.reason));
        }

        Ok(ApprovalRequest { request, spec })
    }

    /// Execute an approved tool without a cancellation token (sync path).
    ///
    /// For `run_shell` this applies the default wall-clock timeout only.
    pub fn execute(&self, approval: &ApprovalRequest) -> Result<ToolOutput, ToolError> {
        self.execute_with_cancel(approval, None)
    }

    /// Execute an approved tool, optionally honouring a [`CancellationToken`].
    ///
    /// Only `run_shell` special-cases cancel today: when `cancel` is `Some` and
    /// the token is cancelled during the poll loop, the child is killed and
    /// [`ToolError::Cancelled`] is returned. All other tools ignore `cancel`
    /// and fall through to the normal synchronous path.
    pub fn execute_with_cancel(
        &self,
        approval: &ApprovalRequest,
        cancel: Option<&CancellationToken>,
    ) -> Result<ToolOutput, ToolError> {
        let args = &approval.request.args;
        let metadata = approval.metadata()?;
        if metadata.decision == PermissionDecision::Deny {
            return Err(ToolError::SecurityDenied(metadata.reason));
        }
        let output = match approval.request.name.as_str() {
            "read_file" => fs::read_to_string(&metadata.affected_paths[0])
                .map_err(|e| ToolError::Io(e.to_string()))?,
            "write_file" => {
                fs::write(&metadata.affected_paths[0], &args[1])
                    .map_err(|e| ToolError::Io(e.to_string()))?;
                format!(
                    "wrote {} bytes to {}",
                    args[1].len(),
                    metadata.affected_paths[0].display()
                )
            }
            "list_dir" => list_dir(&metadata.affected_paths[0])?,
            "file_meta" | "file_metadata" => file_meta(&metadata.affected_paths[0])?,
            "glob" => glob_paths(&args[0], &metadata.cwd)?,
            "grep" => {
                let pattern = &args[0];
                let search_root = metadata
                    .affected_paths
                    .first()
                    .cloned()
                    .unwrap_or_else(|| metadata.cwd.clone());
                grep_files(pattern, &search_root, &metadata.cwd)?
            }
            "edit_file" => {
                let expected_hash = args.get(3).map(String::as_str);
                edit_file_with_hash(
                    &metadata.affected_paths[0],
                    &args[1],
                    &args[2],
                    expected_hash,
                )?
            }
            "run_shell" => run_shell_cancellable(&args[0], cancel)?,
            "http_get" => return Err(ToolError::NetworkDisabled),
            _ => return Err(ToolError::UnknownTool(approval.request.name.clone())),
        };
        Ok(ToolOutput {
            tool: approval.request.name.clone(),
            output,
        })
    }
}

fn validate_args(spec: &ToolSpec, args: &[String]) -> Result<(), ToolError> {
    match spec.name {
        "read_file" | "list_dir" | "run_shell" | "http_get" | "file_meta" | "file_metadata"
        | "glob"
            if args.is_empty() =>
        {
            Err(ToolError::MissingArgument("value"))
        }
        "write_file" if args.len() < 2 => Err(ToolError::MissingArgument("content")),
        "grep" if args.is_empty() => Err(ToolError::MissingArgument("pattern")),
        "edit_file" if args.len() < 3 => {
            Err(ToolError::MissingArgument("path/old_string/new_string"))
        }
        _ => Ok(()),
    }
}

/// Canonicalized project root. `current_dir` is used directly (not
/// `fs::canonicalize`, which on Windows prefixes `\\?\` and breaks
/// `starts_with` comparisons against non-canonical relative paths).
fn project_root() -> Result<PathBuf, ToolError> {
    std::env::current_dir().map_err(|e| ToolError::Io(e.to_string()))
}

/// Strip the Windows verbatim prefix (`\\?\`) so canonicalized paths compare
/// correctly against non-canonicalized `current_dir` paths via `starts_with`.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(r"\\").join(stripped)
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

fn metadata_for(spec: &ToolSpec, args: &[String]) -> Result<ApprovalMetadata, ToolError> {
    let cwd = project_root()?;
    let affected_paths = match spec.name {
        "read_file" | "write_file" | "list_dir" | "file_meta" | "file_metadata" | "edit_file" => {
            vec![safe_path(&args[0], &cwd)?]
        }
        "glob" => {
            // Pattern must stay relative and inside the project root.
            validate_glob_pattern(&args[0])?;
            vec![cwd.clone()]
        }
        "grep" => {
            let raw = args.get(1).map(String::as_str).unwrap_or(".");
            vec![safe_path(raw, &cwd)?]
        }
        _ => Vec::new(),
    };
    let (decision, risk, reason) = classify(spec, args);
    Ok(ApprovalMetadata {
        decision,
        risk,
        cwd,
        affected_paths,
        reason,
    })
}

/// Canonicalize `raw` against `cwd`, enforce project-root containment for
/// relative inputs, and reject sensitive credential/key paths.
fn safe_path(raw: &str, cwd: &Path) -> Result<PathBuf, ToolError> {
    let input = PathBuf::from(raw);
    let candidate = if input.is_absolute() {
        input.clone()
    } else {
        cwd.join(&input)
    };

    // Canonicalize the existing portion (parent must exist for a file we are
    // about to create) and re-append the leaf so non-existent files still get a
    // normalized absolute path.
    let canonical = strip_verbatim(
        if candidate.exists() {
            fs::canonicalize(&candidate)
        } else {
            let parent = candidate.parent().unwrap_or(cwd);
            fs::canonicalize(parent).map(|p| p.join(candidate.file_name().unwrap_or_default()))
        }
        .map_err(|e| ToolError::Io(e.to_string()))?,
    );

    // Project-root containment: relative inputs must not escape cwd after
    // symlink/traversal normalization.
    if !input.is_absolute() && !canonical.starts_with(cwd) {
        return Err(ToolError::SecurityDenied(
            "path escapes project root".into(),
        ));
    }

    if is_sensitive(&canonical) {
        return Err(ToolError::SecurityDenied("sensitive path".into()));
    }

    Ok(canonical)
}

/// Return true for credential, key, and SSH/AWS config paths that must never be
/// read or written through the tool layer.
fn is_sensitive(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    name == ".env"
        || name.starts_with(".env.")
        || name.ends_with(".key")
        || name.ends_with(".pem")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || normalized.contains("/.ssh/")
        || normalized.contains("/.aws/")
        || normalized.contains("/.config/gcloud/")
        || normalized.contains("credentials")
        || normalized.contains("secrets")
}

/// Produce a (decision, risk, reason) triple for a tool request. Deny is used
/// for hard policy blocks (network, destructive shell). Everything else asks.
fn classify(spec: &ToolSpec, args: &[String]) -> (PermissionDecision, RiskLevel, String) {
    if spec.name == "http_get" {
        return (
            PermissionDecision::Deny,
            RiskLevel::Critical,
            "network disabled".into(),
        );
    }

    if spec.name == "run_shell" && destructive_command(&args[0]) {
        return (
            PermissionDecision::Deny,
            RiskLevel::Critical,
            "destructive shell command blocked".into(),
        );
    }

    let risk = match spec.permission {
        Permission::ReadOnly => RiskLevel::Low,
        Permission::Write => RiskLevel::Medium,
        Permission::Execute => RiskLevel::High,
        Permission::Network => RiskLevel::Critical,
    };

    (
        PermissionDecision::Ask,
        risk,
        format!("{} permission requires approval", spec.permission.label()),
    )
}

/// Match a small denylist of destructive commands across POSIX and Windows
/// shells. This is defense-in-depth, not a sandbox.
fn destructive_command(command: &str) -> bool {
    let c = command.to_ascii_lowercase();
    const DENYLIST: &[&str] = &[
        "rm -rf",
        "rmdir /s",
        "del /f",
        "format ",
        "shutdown",
        "reboot",
        "mkfs",
        "diskpart",
        "git reset --hard",
        "git clean -fd",
    ];
    DENYLIST.iter().any(|needle| c.contains(needle))
}

fn list_dir(path: &Path) -> Result<String, ToolError> {
    let mut entries = fs::read_dir(path)
        .map_err(|e| ToolError::Io(e.to_string()))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries.join("\n"))
}

fn file_meta(path: &Path) -> Result<String, ToolError> {
    let meta = fs::metadata(path).map_err(|e| ToolError::Io(e.to_string()))?;
    let kind = if meta.is_dir() {
        "dir"
    } else if meta.is_file() {
        "file"
    } else {
        "other"
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|| "unknown".into());
    Ok(format!(
        "path: {}\nsize: {}\nkind: {}\nis_file: {}\nis_dir: {}\nmodified_unix: {}",
        path.display(),
        meta.len(),
        kind,
        meta.is_file(),
        meta.is_dir(),
        modified
    ))
}

/// Reject absolute patterns and parent-directory escapes before walking.
fn validate_glob_pattern(pattern: &str) -> Result<(), ToolError> {
    let path = Path::new(pattern);
    if path.is_absolute() {
        return Err(ToolError::SecurityDenied(
            "glob pattern must be relative to project root".into(),
        ));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(ToolError::SecurityDenied(
                "glob pattern must not escape project root".into(),
            ));
        }
    }
    if pattern.is_empty() {
        return Err(ToolError::MissingArgument("pattern"));
    }
    Ok(())
}

/// Simple recursive glob under `root`. Supports `*`, `?`, and `**` with std only.
fn glob_paths(pattern: &str, root: &Path) -> Result<String, ToolError> {
    validate_glob_pattern(pattern)?;
    let normalized = pattern.replace('\\', "/");
    let mut matches = Vec::new();
    walk_glob(root, root, &normalized, &mut matches)?;
    matches.sort();
    let truncated = matches.len() > GLOB_RESULT_CAP;
    matches.truncate(GLOB_RESULT_CAP);
    let mut out = matches.join("\n");
    if truncated {
        out.push_str(&format!("\n… truncated to {GLOB_RESULT_CAP} results"));
    }
    if out.is_empty() {
        out = "(no matches)".into();
    }
    Ok(out)
}

fn walk_glob(
    root: &Path,
    current: &Path,
    pattern: &str,
    out: &mut Vec<String>,
) -> Result<(), ToolError> {
    if out.len() >= GLOB_RESULT_CAP {
        return Ok(());
    }
    let entries = match fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.filter_map(Result::ok) {
        if out.len() >= GLOB_RESULT_CAP {
            break;
        }
        let path = entry.path();
        if is_sensitive(&path) {
            continue;
        }
        // Keep results inside project root (defense against odd junctions).
        let canonical = match fs::canonicalize(&path) {
            Ok(p) => strip_verbatim(p),
            Err(_) => continue,
        };
        if !canonical.starts_with(root) {
            continue;
        }
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if glob_match(pattern, &rel_str) {
            out.push(rel_str.clone());
        }
        if path.is_dir() {
            // Skip heavy / irrelevant trees.
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            walk_glob(root, &path, pattern, out)?;
        }
    }
    Ok(())
}

/// Match a relative path against a glob pattern (`*`, `?`, `**`).
fn glob_match(pattern: &str, path: &str) -> bool {
    glob_match_parts(
        &split_glob_segments(pattern),
        &path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>(),
    )
}

fn split_glob_segments(pattern: &str) -> Vec<&str> {
    pattern.split('/').filter(|s| !s.is_empty()).collect()
}

fn glob_match_parts(pattern: &[&str], path: &[&str]) -> bool {
    match (pattern.first(), path.first()) {
        (None, None) => true,
        (Some(&"**"), _) => {
            // `**` matches zero or more path segments.
            if glob_match_parts(&pattern[1..], path) {
                return true;
            }
            if path.is_empty() {
                return false;
            }
            glob_match_parts(pattern, &path[1..])
        }
        (Some(p), Some(seg)) => {
            segment_match(p, seg) && glob_match_parts(&pattern[1..], &path[1..])
        }
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn segment_match(pattern: &str, segment: &str) -> bool {
    let pb: Vec<char> = pattern.chars().collect();
    let sb: Vec<char> = segment.chars().collect();
    segment_match_chars(&pb, &sb)
}

fn segment_match_chars(pattern: &[char], segment: &[char]) -> bool {
    match (pattern.first(), segment.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            // `*` matches any run of chars within one segment.
            if segment_match_chars(&pattern[1..], segment) {
                return true;
            }
            if segment.is_empty() {
                return false;
            }
            segment_match_chars(pattern, &segment[1..])
        }
        (Some('?'), Some(_)) => segment_match_chars(&pattern[1..], &segment[1..]),
        (Some(pc), Some(sc)) if pc == sc => segment_match_chars(&pattern[1..], &segment[1..]),
        _ => false,
    }
}

fn grep_files(pattern: &str, search_root: &Path, project_root: &Path) -> Result<String, ToolError> {
    if pattern.is_empty() {
        return Err(ToolError::MissingArgument("pattern"));
    }
    let mut matches = Vec::new();
    let mut truncated = false;
    grep_walk(
        search_root,
        project_root,
        pattern,
        &mut matches,
        &mut truncated,
    )?;
    if matches.is_empty() {
        return Ok("(no matches)".into());
    }
    let mut out = matches.join("\n");
    if truncated {
        out.push_str(&format!("\n… truncated to {GREP_MATCH_CAP} matches"));
    }
    Ok(out)
}

fn grep_walk(
    current: &Path,
    project_root: &Path,
    pattern: &str,
    out: &mut Vec<String>,
    truncated: &mut bool,
) -> Result<(), ToolError> {
    if out.len() >= GREP_MATCH_CAP {
        *truncated = true;
        return Ok(());
    }

    let meta = match fs::metadata(current) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    if meta.is_file() {
        grep_one_file(current, project_root, pattern, out, truncated)?;
        return Ok(());
    }

    if !meta.is_dir() {
        return Ok(());
    }

    let entries = match fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };
    for entry in entries.filter_map(Result::ok) {
        if out.len() >= GREP_MATCH_CAP {
            *truncated = true;
            break;
        }
        let path = entry.path();
        if is_sensitive(&path) {
            continue;
        }
        if let Ok(canonical) = fs::canonicalize(&path) {
            let canonical = strip_verbatim(canonical);
            if !canonical.starts_with(project_root) {
                continue;
            }
        }
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            grep_walk(&path, project_root, pattern, out, truncated)?;
        } else {
            grep_one_file(&path, project_root, pattern, out, truncated)?;
        }
    }
    Ok(())
}

fn grep_one_file(
    path: &Path,
    project_root: &Path,
    pattern: &str,
    out: &mut Vec<String>,
    truncated: &mut bool,
) -> Result<(), ToolError> {
    if out.len() >= GREP_MATCH_CAP {
        *truncated = true;
        return Ok(());
    }
    if is_sensitive(path) {
        return Ok(());
    }
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if !meta.is_file() || meta.len() > GREP_MAX_FILE_BYTES {
        return Ok(());
    }
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };
    if is_binary_ish(&bytes) {
        return Ok(());
    }
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return Ok(()),
    };
    let display = path
        .strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    for (idx, line) in text.lines().enumerate() {
        if out.len() >= GREP_MATCH_CAP {
            *truncated = true;
            break;
        }
        if line.contains(pattern) {
            out.push(format!("{display}:{}:{line}", idx + 1));
        }
    }
    Ok(())
}

fn is_binary_ish(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return true;
    }
    // High ratio of non-text control bytes ⇒ treat as binary.
    let sample = &bytes[..bytes.len().min(8192)];
    if sample.is_empty() {
        return false;
    }
    let nontext = sample
        .iter()
        .filter(|&&b| b < 0x09 || (b > 0x0d && b < 0x20) || b == 0x7f)
        .count();
    nontext * 100 / sample.len() > 10
}

/// Content hash of raw file bytes.
///
/// Format: `dh64:<16 lowercase hex digits>` — non-cryptographic fingerprint
/// from [`std::collections::hash_map::DefaultHasher`] (SipHash-1-3) over
/// `len` then the full byte slice. Suitable for stale-edit detection within a
/// session, not for integrity against adversaries.
pub fn content_hash_of(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write_usize(bytes.len());
    hasher.write(bytes);
    format!("dh64:{:016x}", hasher.finish())
}

/// Dominant newline style in `text`: CRLF if any `\r\n` is present, else LF.
fn detect_newline_style(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Rewrite bare/mixed newlines in `s` to match `nl` (`"\n"` or `"\r\n"`).
fn normalize_newlines_to(s: &str, nl: &str) -> String {
    // First collapse CRLF → LF so we never double-convert, then expand.
    let lf_only = s.replace("\r\n", "\n").replace('\r', "\n");
    if nl == "\r\n" {
        lf_only.replace('\n', "\r\n")
    } else {
        lf_only
    }
}

/// Build a simple unified-diff hunk for the unique `old` → `new` replacement
/// inside `before`. Line-based; not a full Myers diff — enough to inspect the
/// change before/after acceptance.
fn unified_diff_for_replace(path_display: &str, before: &str, old: &str, new: &str) -> String {
    let pos = before.find(old).unwrap_or(0);
    let start_line = before[..pos].matches('\n').count() + 1;

    // Split on '\n' but keep visual lines; strip trailing '\r' so CRLF files
    // still show clean unified-diff lines.
    let split_lines = |s: &str| -> Vec<String> {
        if s.is_empty() {
            return Vec::new();
        }
        s.split('\n')
            .map(|line| line.trim_end_matches('\r').to_string())
            .collect()
    };
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    // split produces a trailing empty element when s ends with '\n'; drop it
    // for hunk counts so "a\n" is one line, matching typical diff tools.
    let trim_trailing_empty = |lines: Vec<String>| -> Vec<String> {
        let mut lines = lines;
        if lines.last().is_some_and(|l| l.is_empty()) && lines.len() > 1 {
            lines.pop();
        }
        lines
    };
    let old_lines = trim_trailing_empty(old_lines);
    let new_lines = trim_trailing_empty(new_lines);
    let old_n = old_lines.len().max(1);
    let new_n = new_lines.len().max(1);

    let mut out = String::new();
    out.push_str(&format!("--- a/{path_display}\n"));
    out.push_str(&format!("+++ b/{path_display}\n"));
    out.push_str(&format!(
        "@@ -{start_line},{old_n} +{start_line},{new_n} @@\n"
    ));
    if old_lines.is_empty() {
        out.push_str("-\n");
    } else {
        for line in &old_lines {
            out.push('-');
            out.push_str(line);
            out.push('\n');
        }
    }
    if new_lines.is_empty() {
        out.push_str("+\n");
    } else {
        for line in &new_lines {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Replace exactly one occurrence of `old` with `new`.
///
/// When `expected_hash` is `Some`, the on-disk content hash must match or the
/// edit is rejected as stale (`ToolError::EditFailed`). When `None`, behaves
/// like a plain unique replace but still reports hashes.
///
/// Preserves the file's dominant line-ending style (CRLF vs LF) by normalizing
/// the replacement text and writing raw bytes. Output includes `before_hash`,
/// `after_hash`, a line-ending note, and a unified diff of the hunk.
pub fn edit_file_with_hash(
    path: &Path,
    old: &str,
    new: &str,
    expected_hash: Option<&str>,
) -> Result<String, ToolError> {
    if old.is_empty() {
        return Err(ToolError::EditFailed("old_string must not be empty".into()));
    }

    let raw = fs::read(path).map_err(|e| ToolError::Io(e.to_string()))?;
    if raw.contains(&0) {
        return Err(ToolError::EditFailed(
            "refusing to edit binary file (NUL byte present)".into(),
        ));
    }
    let original = String::from_utf8(raw)
        .map_err(|e| ToolError::EditFailed(format!("file is not valid UTF-8: {e}")))?;
    let before_hash = content_hash_of(original.as_bytes());

    if let Some(expected) = expected_hash
        && expected != before_hash
    {
        return Err(ToolError::EditFailed(format!(
            "stale content hash: expected {expected} got {before_hash}"
        )));
    }

    let occurrences = original.matches(old).count();
    match occurrences {
        0 => {
            return Err(ToolError::EditFailed("old_string not found in file".into()));
        }
        1 => {}
        n => {
            return Err(ToolError::EditFailed(format!(
                "old_string is ambiguous ({n} occurrences); refuse to edit"
            )));
        }
    }

    let nl = detect_newline_style(&original);
    // Keep `old` exact (must match on-disk bytes). Normalize only `new` so
    // inserted text follows the file's line-ending convention.
    let new_normalized = normalize_newlines_to(new, nl);
    let updated = original.replacen(old, &new_normalized, 1);

    // TOCTOU: refuse if the file changed under us between read and write.
    let recheck_raw = fs::read(path).map_err(|e| ToolError::Io(e.to_string()))?;
    let recheck_hash = content_hash_of(&recheck_raw);
    if recheck_hash != before_hash {
        return Err(ToolError::EditFailed(format!(
            "file changed since read (stale edit rejected): was {before_hash} now {recheck_hash}"
        )));
    }
    if recheck_raw.as_slice() != original.as_bytes() {
        return Err(ToolError::EditFailed(
            "file content mismatch before write".into(),
        ));
    }

    fs::write(path, updated.as_bytes()).map_err(|e| ToolError::Io(e.to_string()))?;
    let after_hash = content_hash_of(updated.as_bytes());

    let path_display = path.display().to_string();
    let ending_note = if nl == "\r\n" {
        "line_endings: CRLF (preserved)"
    } else {
        "line_endings: LF"
    };
    let diff = unified_diff_for_replace(&path_display, &original, old, &new_normalized);

    Ok(format!(
        "edited {path_display}\nbefore_hash: {before_hash}\nafter_hash: {after_hash}\n{ending_note}\nreplaced {} bytes with {} bytes\n{diff}",
        old.len(),
        new_normalized.len(),
    ))
}

/// Resolve the wall-clock timeout for `run_shell`.
///
/// Reads `LUMINUS_SHELL_TIMEOUT_SECS` when set to a positive integer; otherwise
/// uses [`DEFAULT_SHELL_TIMEOUT_SECS`] (30s). Invalid or zero values fall back
/// to the default.
pub fn shell_timeout() -> Duration {
    match std::env::var("LUMINUS_SHELL_TIMEOUT_SECS") {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(secs) if secs > 0 => Duration::from_secs(secs),
            _ => Duration::from_secs(DEFAULT_SHELL_TIMEOUT_SECS),
        },
        Err(_) => Duration::from_secs(DEFAULT_SHELL_TIMEOUT_SECS),
    }
}

/// Run one shell command with the configured default timeout ([`shell_timeout`])
/// and an optional [`CancellationToken`].
///
/// On timeout the child process is killed (`Child::kill`) and
/// [`ToolError::Timeout`] is returned. When a cancel token is provided and
/// fires during the poll loop, the child is killed and
/// [`ToolError::Cancelled`] is returned.
fn run_shell_cancellable(
    command: &str,
    cancel: Option<&CancellationToken>,
) -> Result<String, ToolError> {
    run_shell_with_timeout_cancellable(command, shell_timeout(), cancel)
}

/// Run one shell command, killing the child if it exceeds `timeout`.
///
/// Wrapper around [`run_shell_with_timeout_cancellable`] with no cancel token,
/// kept for API stability and existing callers/tests.
///
/// Destructive denylist / security checks remain in [`ToolRegistry::prepare`]
/// and are not re-checked here — callers must only invoke this after approval.
pub fn run_shell_with_timeout(command: &str, timeout: Duration) -> Result<String, ToolError> {
    run_shell_with_timeout_cancellable(command, timeout, None)
}

/// Run one shell command, killing the child on timeout **or** cancel.
///
/// Std-only: spawn with piped stdio, drain stdout/stderr on helper threads,
/// poll `try_wait` every [`SHELL_WAIT_POLL`] until exit, deadline, or cancel.
/// When `cancel` is `Some` and `is_cancelled()` during the poll loop, the
/// child is killed and [`ToolError::Cancelled`] is returned. On timeout,
/// [`ToolError::Timeout`] is returned instead.
///
/// On Windows, `kill()` on the direct child is sufficient (process-group kill
/// is harder and not required here).
pub fn run_shell_with_timeout_cancellable(
    command: &str,
    timeout: Duration,
    cancel: Option<&CancellationToken>,
) -> Result<String, ToolError> {
    // Honour a pre-cancelled token before spawning so callers can short-circuit
    // without leaking a child process.
    if cancel.is_some_and(|c| c.is_cancelled()) {
        return Err(ToolError::Cancelled(
            "shell command cancelled before start".into(),
        ));
    }

    #[cfg(windows)]
    let mut child = Command::new("cmd")
        .args(["/C", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::Process(e.to_string()))?;

    #[cfg(not(windows))]
    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ToolError::Process(e.to_string()))?;

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::Process("failed to capture stdout".into()))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::Process("failed to capture stderr".into()))?;

    let (tx_out, rx_out) = mpsc::channel::<Vec<u8>>();
    let (tx_err, rx_err) = mpsc::channel::<Vec<u8>>();

    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut reader = stdout_pipe;
        let _ = reader.read_to_end(&mut buf);
        let _ = tx_out.send(buf);
    });
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut reader = stderr_pipe;
        let _ = reader.read_to_end(&mut buf);
        let _ = tx_err.send(buf);
    });

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout =
                    String::from_utf8_lossy(&rx_out.recv().unwrap_or_default()).into_owned();
                let stderr =
                    String::from_utf8_lossy(&rx_err.recv().unwrap_or_default()).into_owned();
                if status.success() {
                    return Ok(stdout);
                }
                return Err(ToolError::Process(if stderr.trim().is_empty() {
                    stdout
                } else {
                    stderr
                }));
            }
            Ok(None) => {
                if cancel.is_some_and(|c| c.is_cancelled()) {
                    // Best-effort kill of the direct child. On Windows this is
                    // enough for typical cmd /C trees; process-group kill is
                    // out of scope for this slice.
                    let _ = child.kill();
                    let _ = child.wait();
                    // Drain readers so helper threads do not block forever.
                    let _ = rx_out.recv_timeout(Duration::from_millis(200));
                    let _ = rx_err.recv_timeout(Duration::from_millis(200));
                    return Err(ToolError::Cancelled("shell command cancelled".into()));
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = rx_out.recv_timeout(Duration::from_millis(200));
                    let _ = rx_err.recv_timeout(Duration::from_millis(200));
                    return Err(ToolError::Timeout(format!(
                        "shell command exceeded timeout of {}s",
                        timeout.as_secs().max(1)
                    )));
                }
                thread::sleep(SHELL_WAIT_POLL);
            }
            Err(e) => return Err(ToolError::Process(e.to_string())),
        }
    }
}

impl From<io::Error> for ToolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_expected_tools() {
        let names: Vec<_> = ToolRegistry.specs().iter().map(|spec| spec.name).collect();
        assert_eq!(
            names,
            [
                "read_file",
                "write_file",
                "list_dir",
                "file_meta",
                "file_metadata",
                "glob",
                "grep",
                "edit_file",
                "run_shell",
                "http_get"
            ]
        );
    }

    #[test]
    fn prepare_requires_explicit_approval_and_valid_args() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "read_file".into(),
                args: vec!["Cargo.toml".into()],
            })
            .unwrap();
        assert_eq!(approval.spec.permission, Permission::ReadOnly);
        let metadata = approval.metadata().unwrap();
        assert_eq!(metadata.decision, PermissionDecision::Ask);
        assert_eq!(metadata.risk, RiskLevel::Low);
        assert!(!metadata.reason.is_empty());
        assert!(!metadata.affected_paths.is_empty());
        assert!(matches!(
            registry.prepare(ToolRequest {
                name: "read_file".into(),
                args: Vec::new(),
            }),
            Err(ToolError::MissingArgument(_))
        ));
    }

    #[test]
    fn network_tool_is_denied_at_prepare() {
        let registry = ToolRegistry;
        let result = registry.prepare(ToolRequest {
            name: "http_get".into(),
            args: vec!["https://example.test".into()],
        });
        assert!(matches!(result, Err(ToolError::SecurityDenied(_))));
    }

    #[test]
    fn destructive_shell_is_denied_at_prepare() {
        let registry = ToolRegistry;
        let result = registry.prepare(ToolRequest {
            name: "run_shell".into(),
            args: vec!["rm -rf /".into()],
        });
        assert!(
            matches!(result, Err(ToolError::SecurityDenied(ref r)) if r.contains("destructive"))
        );
    }

    #[test]
    fn sensitive_env_path_is_denied() {
        let registry = ToolRegistry;
        let result = registry.prepare(ToolRequest {
            name: "read_file".into(),
            args: vec![".env".into()],
        });
        assert!(matches!(result, Err(ToolError::SecurityDenied(ref r)) if r.contains("sensitive")));
    }

    #[test]
    fn traversal_escape_is_denied() {
        let registry = ToolRegistry;
        let result = registry.prepare(ToolRequest {
            name: "read_file".into(),
            args: vec!["../../../../etc/passwd".into()],
        });
        assert!(matches!(
            result,
            Err(ToolError::SecurityDenied(_)) | Err(ToolError::Io(_))
        ));
    }

    #[test]
    fn approved_read_and_shell_tools_execute() {
        let registry = ToolRegistry;
        let read = registry
            .prepare(ToolRequest {
                name: "read_file".into(),
                args: vec!["Cargo.toml".into()],
            })
            .unwrap();
        let output = registry.execute(&read).unwrap();
        assert!(output.output.contains("[package]"));

        let shell = registry
            .prepare(ToolRequest {
                name: "run_shell".into(),
                args: vec!["printf luminus".into()],
            })
            .unwrap();
        assert_eq!(registry.execute(&shell).unwrap().output.trim(), "luminus");
    }

    #[test]
    fn write_and_list_tools_execute() {
        let registry = ToolRegistry;
        let path = std::env::temp_dir().join(format!("luminus-tool-{}.txt", std::process::id()));
        let path_string = path.to_string_lossy().into_owned();
        let write = registry
            .prepare(ToolRequest {
                name: "write_file".into(),
                args: vec![path_string.clone(), "phase12".into()],
            })
            .unwrap();
        registry.execute(&write).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "phase12");

        let directory = path.parent().unwrap().to_string_lossy().into_owned();
        let list = registry
            .prepare(ToolRequest {
                name: "list_dir".into(),
                args: vec![directory],
            })
            .unwrap();
        assert!(
            registry
                .execute(&list)
                .unwrap()
                .output
                .contains(path.file_name().unwrap().to_string_lossy().as_ref())
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn file_meta_reports_basic_fields() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "file_meta".into(),
                args: vec!["Cargo.toml".into()],
            })
            .unwrap();
        let meta = approval.metadata().unwrap();
        assert_eq!(meta.risk, RiskLevel::Low);
        assert_eq!(meta.affected_paths.len(), 1);
        let output = registry.execute(&approval).unwrap().output;
        assert!(output.contains("kind: file"));
        assert!(output.contains("is_file: true"));
        assert!(output.contains("size:"));
        assert!(output.contains("modified_unix:"));
    }

    #[test]
    fn file_metadata_alias_and_sensitive_deny() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "file_metadata".into(),
                args: vec!["Cargo.toml".into()],
            })
            .unwrap();
        assert!(
            registry
                .execute(&approval)
                .unwrap()
                .output
                .contains("kind:")
        );

        let denied = registry.prepare(ToolRequest {
            name: "file_meta".into(),
            args: vec![".env".into()],
        });
        assert!(matches!(
            denied,
            Err(ToolError::SecurityDenied(ref r)) if r.contains("sensitive")
        ));
    }

    #[test]
    fn glob_finds_cargo_toml_and_blocks_escape() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "glob".into(),
                args: vec!["Cargo.t*".into()],
            })
            .unwrap();
        let output = registry.execute(&approval).unwrap().output;
        assert!(
            output
                .lines()
                .any(|l| l == "Cargo.toml" || l.ends_with("Cargo.toml")),
            "glob should find Cargo.toml; got {output:?}"
        );

        let escape = registry.prepare(ToolRequest {
            name: "glob".into(),
            args: vec!["../**".into()],
        });
        assert!(matches!(escape, Err(ToolError::SecurityDenied(_))));

        let absolute = registry.prepare(ToolRequest {
            name: "glob".into(),
            args: vec![if cfg!(windows) {
                r"C:\Windows\*".into()
            } else {
                "/*".into()
            }],
        });
        assert!(matches!(absolute, Err(ToolError::SecurityDenied(_))));
    }

    #[test]
    fn grep_returns_path_line_content_and_respects_security() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "grep".into(),
                args: vec!["Permission-gated".into(), "src".into()],
            })
            .unwrap();
        let output = registry.execute(&approval).unwrap().output;
        assert!(
            output.contains("tools.rs:") && output.contains("Permission-gated"),
            "grep should report path:line:content; got {output:?}"
        );

        let denied = registry.prepare(ToolRequest {
            name: "grep".into(),
            args: vec!["SECRET".into(), ".env".into()],
        });
        assert!(matches!(
            denied,
            Err(ToolError::SecurityDenied(ref r)) if r.contains("sensitive")
        ));

        assert!(matches!(
            registry.prepare(ToolRequest {
                name: "grep".into(),
                args: Vec::new(),
            }),
            Err(ToolError::MissingArgument(_))
        ));
    }

    #[test]
    fn edit_file_unique_replace_and_ambiguity_guards() {
        let registry = ToolRegistry;
        let path = std::env::temp_dir().join(format!(
            "luminus-edit-{}-{}.txt",
            std::process::id(),
            "unique"
        ));
        fs::write(&path, "alpha beta alpha\n").unwrap();
        let path_string = path.to_string_lossy().into_owned();

        // Ambiguous: "alpha" appears twice.
        let ambiguous = registry
            .prepare(ToolRequest {
                name: "edit_file".into(),
                args: vec![path_string.clone(), "alpha".into(), "ALPHA".into()],
            })
            .unwrap();
        let err = registry.execute(&ambiguous).unwrap_err();
        assert!(
            matches!(err, ToolError::EditFailed(ref r) if r.contains("ambiguous")),
            "expected ambiguous edit failure, got {err}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha beta alpha\n");

        // Missing old_string.
        let missing = registry
            .prepare(ToolRequest {
                name: "edit_file".into(),
                args: vec![path_string.clone(), "does-not-exist".into(), "x".into()],
            })
            .unwrap();
        assert!(matches!(
            registry.execute(&missing),
            Err(ToolError::EditFailed(ref r)) if r.contains("not found")
        ));

        // Unique replace.
        let ok = registry
            .prepare(ToolRequest {
                name: "edit_file".into(),
                args: vec![path_string.clone(), "beta".into(), "BETA".into()],
            })
            .unwrap();
        let meta = ok.metadata().unwrap();
        assert_eq!(meta.risk, RiskLevel::Medium);
        assert_eq!(meta.affected_paths.len(), 1);
        let output = registry.execute(&ok).unwrap().output;
        assert!(
            output.contains(path.file_name().unwrap().to_string_lossy().as_ref())
                || output.contains("edited")
        );
        assert!(
            output.contains("before_hash: dh64:"),
            "expected before_hash in output: {output}"
        );
        assert!(
            output.contains("after_hash: dh64:"),
            "expected after_hash in output: {output}"
        );
        assert!(
            output.contains("--- a/") && output.contains("+++ b/") && output.contains("@@"),
            "expected unified diff markers in output: {output}"
        );
        assert!(
            output.contains("-beta") && output.contains("+BETA"),
            "expected hunk lines in output: {output}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "alpha BETA alpha\n");

        // Sensitive path denied at prepare.
        let sensitive = registry.prepare(ToolRequest {
            name: "edit_file".into(),
            args: vec![".env".into(), "a".into(), "b".into()],
        });
        assert!(matches!(
            sensitive,
            Err(ToolError::SecurityDenied(ref r)) if r.contains("sensitive")
        ));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn glob_match_supports_star_question_and_doublestar() {
        assert!(glob_match("*.rs", "tools.rs"));
        assert!(glob_match("src/*.rs", "src/tools.rs"));
        assert!(glob_match("src/**/*.rs", "src/tui/mod.rs"));
        assert!(glob_match("**/tools.rs", "src/tools.rs"));
        assert!(!glob_match("*.toml", "src/tools.rs"));
        assert!(segment_match("t?ols", "tools"));
        assert!(!segment_match("t?ols", "toools"));
    }

    #[test]
    fn shell_timeout_defaults_and_env_override() {
        // Unset path is hard to assert under parallel tests; validate the
        // positive-parse branch and that invalid values fall back.
        let previous = std::env::var("LUMINUS_SHELL_TIMEOUT_SECS").ok();
        // SAFETY: test-only env mutation; sequential within this test body.
        unsafe {
            std::env::set_var("LUMINUS_SHELL_TIMEOUT_SECS", "12");
        }
        assert_eq!(shell_timeout(), Duration::from_secs(12));
        unsafe {
            std::env::set_var("LUMINUS_SHELL_TIMEOUT_SECS", "0");
        }
        assert_eq!(
            shell_timeout(),
            Duration::from_secs(DEFAULT_SHELL_TIMEOUT_SECS)
        );
        unsafe {
            std::env::set_var("LUMINUS_SHELL_TIMEOUT_SECS", "nope");
        }
        assert_eq!(
            shell_timeout(),
            Duration::from_secs(DEFAULT_SHELL_TIMEOUT_SECS)
        );
        match previous {
            Some(v) => unsafe { std::env::set_var("LUMINUS_SHELL_TIMEOUT_SECS", v) },
            None => unsafe { std::env::remove_var("LUMINUS_SHELL_TIMEOUT_SECS") },
        }
    }

    #[test]
    fn run_shell_fast_command_succeeds() {
        let out = run_shell_with_timeout("echo luminus-timeout-ok", Duration::from_secs(5))
            .expect("fast shell command should succeed");
        assert!(
            out.contains("luminus-timeout-ok"),
            "unexpected stdout: {out:?}"
        );
    }

    #[test]
    fn run_shell_times_out_and_kills_child() {
        let started = Instant::now();
        #[cfg(windows)]
        let cmd = "ping -n 6 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let cmd = "sleep 5";
        let err = run_shell_with_timeout(cmd, Duration::from_secs(1)).unwrap_err();
        assert!(
            matches!(err, ToolError::Timeout(ref m) if m.contains("timeout")),
            "expected ToolError::Timeout, got {err}"
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(4),
            "timeout kill should return well before the long sleep finishes; took {elapsed:?}"
        );
    }
}
