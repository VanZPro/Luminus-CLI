//! Configuration loader: merges global config, project config, and env vars.
//! Secrets (API keys) are stored in the config file on disk but NEVER committed
//! to git. The `.gitignore` excludes `.luminus/` and `.env`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:20128/v1".to_owned(),
            api_key: None,
            model: "BomWaktu".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AppConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    /// Role-to-model overrides (e.g. { "fast": "BomWaktu-fast" })
    #[serde(default)]
    pub models: HashMap<String, String>,
    /// Whether to auto-save learned skills.
    #[serde(default = "default_true")]
    pub auto_improve: bool,
}

fn default_true() -> bool {
    true
}

impl AppConfig {
    /// Load config by merging: global config <- project config <- env overrides.
    pub fn load(project_root: &Path) -> Self {
        let global = Self::load_file(&paths::global_config_path()).unwrap_or_default();
        let project =
            Self::load_file(&paths::project_config_path(project_root)).unwrap_or_default();

        // Merge: project overrides global
        let mut merged = global;
        // Provider: project overrides global if set
        if project.provider.base_url != ProviderConfig::default().base_url {
            merged.provider.base_url = project.provider.base_url;
        }
        if project.provider.api_key.is_some() {
            merged.provider.api_key = project.provider.api_key;
        }
        if !project.provider.model.is_empty()
            && project.provider.model != ProviderConfig::default().model
        {
            merged.provider.model = project.provider.model;
        }
        // Models: merge maps, project wins
        for (k, v) in project.models {
            merged.models.insert(k, v);
        }
        merged.auto_improve = project.auto_improve || merged.auto_improve;

        // Load a Hermes-compatible dotenv file without printing its contents.
        // Explicit `LUMINUS_ENV_FILE` wins; Downloads/.env is supported for migration.
        let env_file = std::env::var_os("LUMINUS_ENV_FILE")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join("Downloads").join(".env")));
        #[allow(clippy::collapsible_if)]
        if let Some(path) = env_file {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        let value = value.trim().trim_matches('"').trim_matches('\'');
                        if !value.is_empty() {
                            unsafe {
                                std::env::set_var(key.trim(), value);
                            }
                        }
                    }
                }
            }
        }

        // Env overrides (highest priority)
        if let Ok(key) =
            std::env::var("OPENAI_API_KEY").or_else(|_| std::env::var("LUMINUS_OPENAI_API_KEY"))
        {
            #[allow(clippy::collapsible_if)]
            if !key.trim().is_empty() {
                merged.provider.api_key = Some(key);
            }
        }
        if let Ok(url) =
            std::env::var("OPENAI_BASE_URL").or_else(|_| std::env::var("LUMINUS_OPENAI_BASE_URL"))
        {
            merged.provider.base_url = url;
        }
        if let Ok(model) =
            std::env::var("OPENAI_MODEL").or_else(|_| std::env::var("LUMINUS_OPENAI_MODEL"))
        {
            merged.provider.model = model;
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

    /// Save config to the project-local `.luminus/config.json`.
    pub fn save_project(&self, project_root: &Path) -> std::io::Result<PathBuf> {
        let dir = paths::project_luminus_dir(project_root);
        paths::ensure_dir(&dir)?;
        let path = paths::project_config_path(project_root);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Save config to the global config path.
    pub fn save_global(&self) -> std::io::Result<PathBuf> {
        let dir = paths::global_data_dir();
        paths::ensure_dir(&dir)?;
        let path = paths::global_config_path();
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, content)?;
        Ok(path)
    }

    /// Redacted view for display — API key replaced with `[REDACTED]`.
    pub fn redacted(&self) -> Self {
        let mut copy = self.clone();
        copy.provider.api_key = copy
            .provider
            .api_key
            .as_ref()
            .map(|_| "[REDACTED]".to_owned());
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_provider_is_local_custom() {
        let cfg = ProviderConfig::default();
        assert_eq!(cfg.base_url, "http://localhost:20128/v1");
        assert_eq!(cfg.model, "BomWaktu");
    }

    #[test]
    fn redacted_hides_api_key() {
        let cfg = AppConfig {
            provider: ProviderConfig {
                base_url: "http://test".into(),
                api_key: Some("secret-key".into()),
                model: "test-model".into(),
            },
            ..Default::default()
        };
        let red = cfg.redacted();
        assert_eq!(red.provider.api_key, Some("[REDACTED]".into()));
    }

    #[test]
    fn save_and_load_project_config() {
        let dir = std::env::temp_dir().join(format!("luminus-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = AppConfig {
            provider: ProviderConfig {
                base_url: "http://test:8080/v1".into(),
                api_key: Some("k".into()),
                model: "m".into(),
            },
            ..Default::default()
        };
        let path = cfg.save_project(&dir).unwrap();
        assert!(path.exists());
        let loaded = AppConfig::load_file(&path).unwrap();
        assert_eq!(loaded.provider.model, "m");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
