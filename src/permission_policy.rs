//! Project-persisted tool permission rules.
//!
//! Stored as JSON at `<project>/.luminus/tool_policy.json` (typically the
//! process current working directory). Writes are atomic: serialize to a
//! `.tmp` sibling then rename into place (same pattern as [`crate::session::Session`]).
//!
//! These rules outlive a single process. [`crate::app::App::clear`] resets
//! **session** allow/deny lists only — project disk rules are left intact.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Persistable allow/deny decision for a single tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolPolicy {
    Allowed,
    Denied,
}

/// On-disk / in-memory map of tool name → project policy.
///
/// `root` is the project directory whose `.luminus/tool_policy.json` is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectToolPolicy {
    tools: HashMap<String, ToolPolicy>,
    root: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ProjectToolPolicyFile {
    #[serde(default)]
    tools: HashMap<String, ToolPolicy>,
}

impl Default for ProjectToolPolicy {
    fn default() -> Self {
        Self {
            tools: HashMap::new(),
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl ProjectToolPolicy {
    /// Empty policy bound to `root` (no disk read).
    pub fn empty(root: impl Into<PathBuf>) -> Self {
        Self {
            tools: HashMap::new(),
            root: root.into(),
        }
    }

    /// Path of the policy file under `project_root`.
    pub fn policy_path(project_root: impl AsRef<Path>) -> PathBuf {
        project_root
            .as_ref()
            .join(".luminus")
            .join("tool_policy.json")
    }

    /// Project root this store is bound to.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Number of tool entries currently in memory.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the in-memory map is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Load from `<project_root>/.luminus/tool_policy.json`.
    ///
    /// Missing file → empty policy bound to `project_root` (not an error).
    pub fn load(project_root: impl AsRef<Path>) -> io::Result<Self> {
        let root = project_root.as_ref().to_path_buf();
        let path = Self::policy_path(&root);
        match fs::read(&path) {
            Ok(bytes) => {
                let file: ProjectToolPolicyFile =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                Ok(Self {
                    tools: file.tools,
                    root,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::empty(root)),
            Err(error) => Err(error),
        }
    }

    /// Atomically write the current map to disk under `self.root`.
    pub fn save(&self) -> io::Result<PathBuf> {
        let directory = self.root.join(".luminus");
        fs::create_dir_all(&directory)?;
        let path = Self::policy_path(&self.root);
        let tmp = path.with_extension("json.tmp");
        let file = ProjectToolPolicyFile {
            tools: self.tools.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Set (or replace) the policy for `tool`.
    pub fn set(&mut self, tool: impl Into<String>, policy: ToolPolicy) {
        self.tools.insert(tool.into(), policy);
    }

    /// Current policy for `tool`, if any.
    pub fn get(&self, tool: &str) -> Option<ToolPolicy> {
        self.tools.get(tool).copied()
    }

    /// Clear all **in-memory** entries. Does not touch disk until [`Self::save`].
    pub fn clear(&mut self) {
        self.tools.clear();
    }

    /// Iterate tool → policy pairs (for tests / diagnostics).
    pub fn iter(&self) -> impl Iterator<Item = (&str, ToolPolicy)> + '_ {
        self.tools.iter().map(|(k, v)| (k.as_str(), *v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-policy-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn load_missing_file_yields_empty_policy() {
        let root = scratch_root("missing");
        let _ = fs::remove_dir_all(&root);
        let policy = ProjectToolPolicy::load(&root).unwrap();
        assert!(policy.is_empty());
        assert_eq!(policy.root(), root.as_path());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn save_load_round_trip() {
        let root = scratch_root("roundtrip");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut policy = ProjectToolPolicy::empty(&root);
        policy.set("read_file", ToolPolicy::Allowed);
        policy.set("run_shell", ToolPolicy::Denied);
        let path = policy.save().unwrap();
        assert!(
            path.ends_with(".luminus/tool_policy.json")
                || path.ends_with(".luminus\\tool_policy.json")
        );
        assert!(path.is_file());

        let loaded = ProjectToolPolicy::load(&root).unwrap();
        assert_eq!(loaded.get("read_file"), Some(ToolPolicy::Allowed));
        assert_eq!(loaded.get("run_shell"), Some(ToolPolicy::Denied));
        assert_eq!(loaded.get("missing"), None);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clear_empties_memory_only_until_save() {
        let root = scratch_root("clear");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut policy = ProjectToolPolicy::empty(&root);
        policy.set("read_file", ToolPolicy::Allowed);
        policy.save().unwrap();

        policy.clear();
        assert!(policy.is_empty());
        // Disk still has the old content until save.
        let loaded = ProjectToolPolicy::load(&root).unwrap();
        assert_eq!(loaded.get("read_file"), Some(ToolPolicy::Allowed));

        let _ = fs::remove_dir_all(root);
    }
}
