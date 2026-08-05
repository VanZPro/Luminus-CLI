//! Permission-gated coding tools.
//!
//! Tools are deliberately small and synchronous in this phase. The registry
//! validates names/arguments and returns an approval request; callers must
//! explicitly approve before invoking a tool.

use std::{fs, io, path::PathBuf, process::Command};

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

    pub fn prepare(&self, request: ToolRequest) -> Result<ApprovalRequest, ToolError> {
        let spec = TOOL_SPECS
            .iter()
            .find(|spec| spec.name == request.name)
            .cloned()
            .ok_or_else(|| ToolError::UnknownTool(request.name.clone()))?;
        validate_args(&spec, &request.args)?;
        Ok(ApprovalRequest { request, spec })
    }

    pub fn execute(&self, approval: &ApprovalRequest) -> Result<ToolOutput, ToolError> {
        let args = &approval.request.args;
        let output = match approval.request.name.as_str() {
            "read_file" => {
                fs::read_to_string(&args[0]).map_err(|e| ToolError::Io(e.to_string()))?
            }
            "write_file" => {
                fs::write(&args[0], &args[1]).map_err(|e| ToolError::Io(e.to_string()))?;
                format!("wrote {} bytes to {}", args[1].len(), args[0])
            }
            "list_dir" => list_dir(&args[0])?,
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

fn list_dir(path: &str) -> Result<String, ToolError> {
    let mut entries = fs::read_dir(PathBuf::from(path))
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
                args: vec!["README.md".into()],
            })
            .unwrap();
        assert_eq!(approval.spec.permission, Permission::ReadOnly);
        assert!(matches!(
            registry.prepare(ToolRequest {
                name: "read_file".into(),
                args: Vec::new(),
            }),
            Err(ToolError::MissingArgument(_))
        ));
    }

    #[test]
    fn network_tool_is_explicitly_disabled() {
        let registry = ToolRegistry;
        let approval = registry
            .prepare(ToolRequest {
                name: "http_get".into(),
                args: vec!["https://example.test".into()],
            })
            .unwrap();
        assert_eq!(registry.execute(&approval), Err(ToolError::NetworkDisabled));
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
