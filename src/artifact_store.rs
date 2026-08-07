//! Disk-backed artifact store for truncated tool outputs (Phase 12E).
//!
//! Artifacts live under `<data_root>/artifacts/<id>.txt`. The data root defaults
//! to [`crate::session::default_root`] and can be overridden for tests or
//! alternate layouts. Writes are atomic (temp file + rename), matching the
//! session persistence pattern.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::tool_output::ArtifactId;

/// Process-global counter used when UUID generation is unavailable; primarily
/// gives deterministic, collision-resistant ids alongside a random component.
static ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Filesystem store for full tool-output payloads that exceed preview bounds.
///
/// Layout: `<data_root>/artifacts/<id>.txt`
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    data_root: PathBuf,
}

impl ArtifactStore {
    /// Build a store rooted at `data_root` (artifacts go in a child `artifacts/` dir).
    pub fn new(data_root: impl Into<PathBuf>) -> Self {
        Self {
            data_root: data_root.into(),
        }
    }

    /// Store under the platform/session default data directory.
    pub fn with_default_root() -> Self {
        Self::new(crate::session::default_root())
    }

    /// Borrow the configured data root.
    pub fn data_root(&self) -> &Path {
        &self.data_root
    }

    /// Directory that holds artifact files (`<data_root>/artifacts`).
    pub fn artifacts_dir(&self) -> PathBuf {
        self.data_root.join("artifacts")
    }

    /// Absolute path for a given artifact id.
    pub fn path_for(&self, id: &ArtifactId) -> PathBuf {
        self.artifacts_dir()
            .join(format!("{}.txt", sanitize_id(id.as_str())))
    }

    /// Persist `full_text` and return a fresh [`ArtifactId`].
    ///
    /// The write is atomic: content is written to a sibling `.tmp` file and then
    /// renamed into place so a crash cannot leave a half-written artifact.
    pub fn save(&self, full_text: &str) -> io::Result<ArtifactId> {
        let dir = self.artifacts_dir();
        fs::create_dir_all(&dir)?;

        let id = next_artifact_id();
        let path = self.path_for(&id);
        let tmp = path.with_extension("txt.tmp");

        fs::write(&tmp, full_text.as_bytes())?;
        // On Windows, rename over an existing target may fail; artifacts are
        // unique by id so this is a fresh path in the normal case.
        fs::rename(&tmp, &path)?;
        Ok(id)
    }

    /// Load the full text of a previously saved artifact.
    pub fn load(&self, id: &ArtifactId) -> io::Result<String> {
        let path = self.path_for(id);
        fs::read_to_string(path)
    }

    /// Delete an artifact file. Returns `true` if a file was removed.
    pub fn delete(&self, id: &ArtifactId) -> io::Result<bool> {
        let path = self.path_for(id);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Whether an artifact file exists on disk for `id`.
    pub fn exists(&self, id: &ArtifactId) -> bool {
        self.path_for(id).is_file()
    }
}

impl Default for ArtifactStore {
    fn default() -> Self {
        Self::with_default_root()
    }
}

fn next_artifact_id() -> ArtifactId {
    let seq = ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Prefer UUID when available for uniqueness across processes; fall back to
    // a sequential + pid composite that is still path-safe.
    let id = uuid::Uuid::new_v4().to_string();
    // Keep a short sequential prefix for human greppability in logs/tests.
    ArtifactId::new(format!("a{seq}-{id}"))
}

/// Restrict id strings used in filenames to a safe character set.
fn sanitize_id(id: &str) -> String {
    let normalized: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if normalized.is_empty() {
        "artifact".into()
    } else {
        normalized.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-artifact-unit-{}-{}",
            label,
            std::process::id()
        ))
    }

    #[test]
    fn save_load_delete_round_trip() {
        let root = scratch_root("roundtrip");
        let _ = fs::remove_dir_all(&root);
        let store = ArtifactStore::new(&root);

        let id = store.save("hello artifact").unwrap();
        assert!(store.exists(&id));
        assert_eq!(store.load(&id).unwrap(), "hello artifact");
        assert!(
            store
                .path_for(&id)
                .ends_with(format!("{}.txt", sanitize_id(id.as_str())))
        );

        assert!(store.delete(&id).unwrap());
        assert!(!store.exists(&id));
        assert!(!store.delete(&id).unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_creates_artifacts_subdirectory() {
        let root = scratch_root("subdir");
        let _ = fs::remove_dir_all(&root);
        let store = ArtifactStore::new(&root);
        let _ = store.save("x").unwrap();
        assert!(store.artifacts_dir().is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitize_id_strips_path_separators() {
        assert_eq!(sanitize_id("../evil"), "___evil");
        assert_eq!(sanitize_id(""), "artifact");
    }
}
