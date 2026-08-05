//! Permission-gated coding tools.
//!
//! Tools are deliberately small and synchronous in this phase. The registry
//! validates names/arguments and returns an approval request; callers must
//! explicitly approve before invoking a tool.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

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
        }
    }
}

impl std::error::Error for ToolError {}

pub const TOOL_SPECS: [ToolSpec; 5] = [
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

    pub fn execute(&self, approval: &ApprovalRequest) -> Result<ToolOutput, ToolError> {
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
            "run_shell" => run_shell(&args[0])?,
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
        "read_file" | "list_dir" | "run_shell" | "http_get" if args.is_empty() => {
            Err(ToolError::MissingArgument("value"))
        }
        "write_file" if args.len() < 2 => Err(ToolError::MissingArgument("content")),
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
    let affected_paths = if matches!(spec.name, "read_file" | "write_file" | "list_dir") {
        vec![safe_path(&args[0], &cwd)?]
    } else {
        Vec::new()
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

fn run_shell(command: &str) -> Result<String, ToolError> {
    #[cfg(windows)]
    let output = Command::new("cmd").args(["/C", command]).output();
    #[cfg(not(windows))]
    let output = Command::new("sh").args(["-c", command]).output();
    let output = output.map_err(|e| ToolError::Process(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        Ok(stdout.into_owned())
    } else {
        Err(ToolError::Process(if stderr.trim().is_empty() {
            stdout.into_owned()
        } else {
            stderr.into_owned()
        }))
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
}
