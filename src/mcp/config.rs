use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(default = "default_mcp_servers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

fn default_mcp_servers() -> HashMap<String, McpServerConfig> {
    HashMap::new()
}

impl McpConfig {
    /// Load MCP config by merging global + project configs.
    pub fn load(project_root: &Path) -> Self {
        let global = Self::load_file(&paths::global_mcp_config_path()).unwrap_or_default();
        let project =
            Self::load_file(&paths::project_mcp_config_path(project_root)).unwrap_or_default();

        let mut merged = global;
        for (name, cfg) in project.mcp_servers {
            merged.mcp_servers.insert(name, cfg);
        }
        merged
    }

    fn load_file(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save MCP config to project-local `.luminus/mcp.json`.
    pub fn save_project(&self, project_root: &Path) -> std::io::Result<PathBuf> {
        let dir = paths::project_luminus_dir(project_root);
        paths::ensure_dir(&dir)?;
        let path = paths::project_mcp_config_path(project_root);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// List server names as a formatted string.
    pub fn list_servers(&self) -> String {
        if self.mcp_servers.is_empty() {
            return "No MCP servers configured. Add servers in .luminus/mcp.json or %LOCALAPPDATA%/Luminus/mcp.json".to_owned();
        }
        let mut lines = vec!["MCP Servers:".to_owned()];
        for (name, cfg) in &self.mcp_servers {
            lines.push(format!(
                "  {} -> {} {}",
                name,
                cfg.command,
                cfg.args.join(" ")
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_empty_returns_default() {
        let dir = std::env::temp_dir().join(format!("mcp-test-{}", std::process::id()));
        let config = McpConfig::load(&dir);
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn save_and_load_project() {
        let dir = std::env::temp_dir().join(format!("mcp-sv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut config = McpConfig::default();
        config.mcp_servers.insert(
            "test-server".into(),
            McpServerConfig {
                command: "node".into(),
                args: vec!["server.js".into()],
                env: HashMap::new(),
            },
        );
        let path = config.save_project(&dir).unwrap();
        assert!(path.exists());
        let loaded = McpConfig::load(&dir);
        assert_eq!(loaded.mcp_servers.len(), 1);
        assert!(loaded.mcp_servers.contains_key("test-server"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
