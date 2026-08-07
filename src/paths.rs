//! Canonical directory layout for Luminus.
//!
//! Global root: `%LOCALAPPDATA%/Luminus` (Windows) or `~/.local/share/luminus` (Unix).
//! Project root: `<cwd>/.luminus/`.
//!
//! Resolution order (highest priority first):
//!   1. `LUMINUS_DATA_DIR` env override
//!   2. Platform-specific user data dir
//!   3. Fallback `~/.luminus`

use std::path::{Path, PathBuf};

/// Returns the global data root.
pub fn global_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LUMINUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("Luminus");
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("luminus");
        }
    }
    // Ultimate fallback
    dirs_fallback()
}

fn dirs_fallback() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_owned());
    PathBuf::from(home).join(".luminus")
}

/// Global skills directory.
pub fn global_skills_dir() -> PathBuf {
    global_data_dir().join("skills")
}

/// Global plugins directory.
pub fn global_plugins_dir() -> PathBuf {
    global_data_dir().join("plugins")
}

/// Global config file path.
pub fn global_config_path() -> PathBuf {
    global_data_dir().join("config.json")
}

/// Global memory file path.
pub fn global_memory_path() -> PathBuf {
    global_data_dir().join("memory.json")
}

/// Global MCP servers config path.
pub fn global_mcp_config_path() -> PathBuf {
    global_data_dir().join("mcp.json")
}

/// Global sessions directory.
pub fn global_sessions_dir() -> PathBuf {
    global_data_dir().join("sessions")
}

/// Project-local `.luminus/` root relative to the given project root.
pub fn project_luminus_dir(project_root: &Path) -> PathBuf {
    project_root.join(".luminus")
}

/// Project-local skills directory.
pub fn project_skills_dir(project_root: &Path) -> PathBuf {
    project_luminus_dir(project_root).join("skills")
}

/// Project-local config file.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_luminus_dir(project_root).join("config.json")
}

/// Project-local MCP config.
pub fn project_mcp_config_path(project_root: &Path) -> PathBuf {
    project_luminus_dir(project_root).join("mcp.json")
}

/// Project-local artifacts directory.
pub fn project_artifacts_dir(project_root: &Path) -> PathBuf {
    project_luminus_dir(project_root).join("artifacts")
}

/// Project-local tool policy file.
pub fn project_tool_policy_path(project_root: &Path) -> PathBuf {
    project_luminus_dir(project_root).join("tool_policy.json")
}

/// Ensure a directory exists, creating it recursively if needed.
pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_data_dir_returns_path() {
        let dir = global_data_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn project_paths_are_under_dot_luminus() {
        let root = PathBuf::from("/tmp/myproject");
        assert!(project_skills_dir(&root).starts_with(root.join(".luminus")));
        assert!(project_config_path(&root).starts_with(root.join(".luminus")));
        assert!(project_mcp_config_path(&root).starts_with(root.join(".luminus")));
    }
}
