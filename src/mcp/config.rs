use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

impl McpConfig {
    pub fn load_from_dir(dir: &Path) -> std::io::Result<Self> {
        let path = dir.join(".luminus").join("mcp.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let config: Self = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }

    pub fn save_to_dir(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let target_dir = dir.join(".luminus");
        std::fs::create_dir_all(&target_dir)?;
        let path = target_dir.join("mcp.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }
}
