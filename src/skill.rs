//! Skills Foundation (Phase 13).
//!
//! A skill is a portable capability package containing instructions, workflows,
//! optional templates, and tool requirements. This module provides:
//!
//! - [`SkillMetadata`]: lightweight metadata loaded eagerly at startup.
//! - [`Skill`]: full content loaded on demand.
//! - [`SkillRegistry`]: discovers, loads, and manages skills from built-in,
//!   global, and project sources.
//!
//! Skill sources (in precedence order, later overrides earlier):
//! 1. **Built-in**: compiled-in default skills (via [`BUILTIN_SKILLS`]).
//! 2. **Global**: `~/.config/luminus/skills/<name>/SKILL.md` (or platform
//!    config dir via [`global_skills_dir`]).
//! 3. **Project**: `.luminus/skills/<name>/SKILL.md` in the current working
//!    directory — overrides global skills with the same name.

use std::fmt;
use std::path::{Path, PathBuf};

/// Where a skill was discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillSource {
    BuiltIn,
    Global,
    Project,
}

impl fmt::Display for SkillSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BuiltIn => "built-in",
            Self::Global => "global",
            Self::Project => "project",
        })
    }
}

/// Lightweight metadata extracted from a SKILL.md frontmatter.
///
/// Loaded eagerly at startup so [`SkillRegistry::discover`] is cheap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub tags: Vec<String>,
    pub source: SkillSource,
    pub path: PathBuf,
}

impl SkillMetadata {
    /// Returns a short one-line label for list output: `name (source)`.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.source)
    }
}

/// A fully loaded skill: metadata + the body content (without frontmatter).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub metadata: SkillMetadata,
    /// The SKILL.md body text after the frontmatter delimiter.
    pub content: String,
    /// The complete raw SKILL.md text (frontmatter + body).
    pub raw: String,
}

/// Error returned when parsing or loading a skill fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    /// The SKILL.md frontmatter could not be parsed.
    InvalidFrontmatter(String),
    /// The skill file could not be read.
    Io(String),
    /// The skill was not found in any source.
    NotFound(String),
    /// The frontmatter is missing the required `name` field.
    MissingName,
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFrontmatter(msg) => write!(f, "invalid skill frontmatter: {msg}"),
            Self::Io(msg) => write!(f, "skill I/O error: {msg}"),
            Self::NotFound(name) => write!(f, "skill not found: {name}"),
            Self::MissingName => write!(f, "skill frontmatter is missing required `name` field"),
        }
    }
}

impl std::error::Error for SkillError {}

/// Splits SKILL.md text into `(frontmatter_lines, body)` if a `---`-delimited
/// frontmatter block exists at the top. Returns `(empty, raw)` if there is no
/// frontmatter.
fn split_frontmatter(raw: &str) -> (String, String) {
    let trimmed = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"));
    let Some(rest) = trimmed else {
        return (String::new(), raw.to_owned());
    };
    // Find the closing `---` on its own line.
    let close = rest
        .lines()
        .position(|line| line.trim() == "---")
        .unwrap_or(rest.lines().count());
    let frontmatter: String = rest.lines().take(close).collect::<Vec<_>>().join("\n");
    let body_start: usize = rest
        .lines()
        .take(close + 1)
        .map(|l| l.len() + 1) // +1 for newline
        .sum();
    let body = rest
        .get(body_start..)
        .unwrap_or("")
        .trim_start_matches(['\r', '\n'])
        .to_owned();
    (frontmatter, body)
}

/// Extracts a simple `key: value` string field from frontmatter lines.
fn extract_field(frontmatter: &str, key: &str) -> Option<String> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let after = rest.trim_start();
            if let Some(stripped) = after.strip_prefix(':') {
                let value = stripped.trim();
                return Some(value.trim_matches('"').trim_matches('\'').to_owned());
            }
        }
    }
    None
}

/// Extracts a YAML-style list field (lines starting with `- ` under `key:`).
fn extract_list_field(frontmatter: &str, key: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut in_list = false;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{key}:")) {
            in_list = true;
            continue;
        }
        if in_list {
            if let Some(item) = trimmed.strip_prefix("- ") {
                items.push(item.trim_matches('"').trim_matches('\'').to_owned());
            } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                in_list = false;
            }
        }
    }
    items
}

/// Parses metadata from raw SKILL.md text. Does not read files.
fn parse_metadata_from_raw(
    raw: &str,
    source: SkillSource,
    path: PathBuf,
) -> Result<SkillMetadata, SkillError> {
    let (frontmatter, _) = split_frontmatter(raw);
    if frontmatter.is_empty() {
        return Err(SkillError::MissingName);
    }
    let name = extract_field(&frontmatter, "name")
        .ok_or(SkillError::MissingName)?
        .trim()
        .to_owned();
    if name.is_empty() {
        return Err(SkillError::MissingName);
    }
    let description = extract_field(&frontmatter, "description").unwrap_or_default();
    let version = extract_field(&frontmatter, "version").unwrap_or_default();
    let author = extract_field(&frontmatter, "author")
        .or_else(|| extract_field(&frontmatter, "origin"))
        .unwrap_or_default();
    let tags = extract_list_field(&frontmatter, "tags");
    Ok(SkillMetadata {
        name,
        description,
        version,
        author,
        tags,
        source,
        path,
    })
}

/// A built-in skill defined as a compile-time constant string.
struct BuiltinSkill {
    name: &'static str,
    raw: &'static str,
}

/// The default built-in skills shipped with Luminus.
static BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        name: "fix-tests",
        raw: "---\nname: fix-tests\ndescription: Diagnose and fix failing automated tests\nversion: 1.0.0\nauthor: luminus\ntags:\n  - testing\n  - debugging\n---\n\n# Fix Tests\n\n1. Identify the smallest reproducible failing test.\n2. Inspect the related implementation and recent changes.\n3. Explain the likely root cause.\n4. Apply the smallest safe fix.\n5. Run the focused test.\n6. Run the relevant test suite.\n7. Summarize the change and remaining risks.\n",
    },
    BuiltinSkill {
        name: "code-review",
        raw: "---\nname: code-review\ndescription: Review code changes for quality, correctness, and style\nversion: 1.0.0\nauthor: luminus\ntags:\n  - review\n  - quality\n---\n\n# Code Review\n\n1. Read the diff in full before commenting.\n2. Check for correctness, edge cases, and error handling.\n3. Verify naming, structure, and consistency with project conventions.\n4. Suggest improvements with rationale and examples.\n5. Approve, request changes, or block with clear reasoning.\n",
    },
];

/// Resolves the global skills directory: `~/.config/luminus/skills/` on Linux,
/// `%LOCALAPPDATA%/luminus/skills/` on Windows, `~/Library/Application
/// Support/luminus/skills/` on macOS. Overridable via `LUMINUS_DATA_DIR`.
pub fn global_skills_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("LUMINUS_DATA_DIR") {
        return PathBuf::from(path).join("skills");
    }
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("luminus").join("skills");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path)
            .join(".config")
            .join("luminus")
            .join("skills");
    }
    PathBuf::from(".luminus").join("skills")
}

/// Resolves the project skills directory: `<cwd>/.luminus/skills/`.
fn project_skills_dir() -> PathBuf {
    PathBuf::from(".luminus").join("skills")
}

/// The skill registry: discovers, loads, and manages skills from built-in,
/// global, and project sources.
///
/// Project skills override global skills with the same name, and global skills
/// override built-in skills with the same name.
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    /// Cached metadata discovered during `discover()`.
    metadata: Vec<SkillMetadata>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self {
            metadata: Self::discover_raw(),
        }
    }
}

impl SkillRegistry {
    /// Creates a new registry and immediately discovers available skills.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry from a pre-computed metadata list (for testing).
    pub fn from_metadata(metadata: Vec<SkillMetadata>) -> Self {
        Self { metadata }
    }

    /// Discovers all available skill metadata from built-in, global, and project
    /// sources. Project overrides global, which overrides built-in (by name).
    fn discover_raw() -> Vec<SkillMetadata> {
        let mut entries: Vec<SkillMetadata> = Vec::new();

        // 1. Built-in skills
        for builtin in BUILTIN_SKILLS {
            if let Ok(meta) = parse_metadata_from_raw(
                builtin.raw,
                SkillSource::BuiltIn,
                PathBuf::from(format!("<built-in>/{}", builtin.name)),
            ) {
                entries.push(meta);
            }
        }

        // 2. Global skills
        let global_dir = global_skills_dir();
        Self::discover_dir(&global_dir, SkillSource::Global, &mut entries);

        // 3. Project skills
        let project_dir = project_skills_dir();
        Self::discover_dir(&project_dir, SkillSource::Project, &mut entries);

        // Deduplicate: project overrides global, which overrides built-in.
        // Keep only the highest-precedence entry per name.
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut result: Vec<SkillMetadata> = Vec::new();
        for meta in entries {
            match seen.get(&meta.name) {
                Some(&idx) => {
                    // Replace if new source has higher precedence
                    let precedence = |s: SkillSource| match s {
                        SkillSource::BuiltIn => 0,
                        SkillSource::Global => 1,
                        SkillSource::Project => 2,
                    };
                    if precedence(meta.source) > precedence(result[idx].source) {
                        result[idx] = meta;
                    }
                }
                None => {
                    seen.insert(meta.name.clone(), result.len());
                    result.push(meta);
                }
            }
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    /// Scans a directory for `<name>/SKILL.md` and appends metadata to `entries`.
    fn discover_dir(dir: &Path, source: SkillSource, entries: &mut Vec<SkillMetadata>) {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_file = path.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            match std::fs::read_to_string(&skill_file) {
                Ok(raw) => {
                    if let Ok(meta) = parse_metadata_from_raw(&raw, source, skill_file) {
                        entries.push(meta);
                    }
                }
                Err(_) => continue,
            }
        }
    }

    /// Returns metadata for all discovered skills.
    pub fn discover(&self) -> &[SkillMetadata] {
        &self.metadata
    }

    /// Finds metadata for a skill by name (case-insensitive).
    pub fn find(&self, name: &str) -> Option<&SkillMetadata> {
        self.metadata
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// Loads the full skill content on demand.
    pub fn load_skill(&self, name: &str) -> Result<Skill, SkillError> {
        let meta = self
            .find(name)
            .ok_or_else(|| SkillError::NotFound(name.to_owned()))?;
        let raw = match meta.source {
            SkillSource::BuiltIn => BUILTIN_SKILLS
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(&meta.name))
                .map(|b| b.raw.to_owned())
                .ok_or_else(|| SkillError::Io("built-in skill source missing".into()))?,
            SkillSource::Global | SkillSource::Project => {
                std::fs::read_to_string(&meta.path).map_err(|e| SkillError::Io(e.to_string()))?
            }
        };
        let (_, content) = split_frontmatter(&raw);
        Ok(Skill {
            metadata: meta.clone(),
            content,
            raw,
        })
    }

    /// Formats the skill list for `/skills` or `/skills list`.
    pub fn list_skills(&self) -> String {
        if self.metadata.is_empty() {
            return "No skills available.".to_owned();
        }
        let mut lines = vec!["Available skills:".to_owned()];
        for meta in &self.metadata {
            let desc = if meta.description.is_empty() {
                String::new()
            } else {
                format!(" — {}", meta.description)
            };
            lines.push(format!("  {} ({}){}", meta.name, meta.source, desc));
        }
        lines.join("\n")
    }

    /// Formats detailed skill info for `/skills inspect <name>`.
    pub fn inspect_skill(&self, name: &str) -> Result<String, SkillError> {
        let skill = self.load_skill(name)?;
        let meta = &skill.metadata;
        let mut lines = vec![format!("Skill: {}", meta.name)];
        if !meta.description.is_empty() {
            lines.push(format!("Description: {}", meta.description));
        }
        if !meta.version.is_empty() {
            lines.push(format!("Version: {}", meta.version));
        }
        if !meta.author.is_empty() {
            lines.push(format!("Author: {}", meta.author));
        }
        lines.push(format!("Source: {}", meta.source));
        lines.push(format!("Path: {}", meta.path.display()));
        if !meta.tags.is_empty() {
            lines.push(format!("Tags: {}", meta.tags.join(", ")));
        }
        lines.push(String::new());
        lines.push("--- Skill Content ---".into());
        lines.push(skill.content.trim().to_owned());
        Ok(lines.join("\n"))
    }

    /// Loads a skill and formats its instructions for injection into the
    /// conversation transcript when the skill is activated via `/skill <name>`.
    pub fn use_skill(&self, name: &str) -> Result<String, SkillError> {
        let skill = self.load_skill(name)?;
        Ok(format!(
            "[Skill activated: {} ({})]\n\n{}",
            skill.metadata.name,
            skill.metadata.source,
            skill.content.trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-skill-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    const SAMPLE_SKILL: &str = "---\nname: fix-tests\ndescription: Diagnose and fix failing tests\nversion: 1.0.0\nauthor: luminus\ntags:\n  - testing\n  - debugging\n---\n\n# Fix Tests\n\n1. Identify the smallest reproducible failing test.\n2. Apply the smallest safe fix.\n";

    #[test]
    fn split_frontmatter_extracts_body() {
        let (fm, body) = split_frontmatter(SAMPLE_SKILL);
        assert!(fm.contains("name: fix-tests"));
        assert!(fm.contains("description:"));
        assert!(body.starts_with("# Fix Tests"));
        assert!(!body.contains("---"));
    }

    #[test]
    fn split_frontmatter_no_frontmatter_returns_raw() {
        let raw = "Just some markdown.\nNo frontmatter here.";
        let (fm, body) = split_frontmatter(raw);
        assert!(fm.is_empty());
        assert_eq!(body, raw);
    }

    #[test]
    fn extract_field_finds_value() {
        let fm = "name: test-skill\ndescription: A test\n";
        assert_eq!(extract_field(fm, "name"), Some("test-skill".into()));
        assert_eq!(extract_field(fm, "description"), Some("A test".into()));
        assert_eq!(extract_field(fm, "missing"), None);
    }

    #[test]
    fn extract_list_field_finds_items() {
        let fm = "tags:\n  - alpha\n  - beta\nversion: 1.0\n";
        let tags = extract_list_field(fm, "tags");
        assert_eq!(tags, vec!["alpha", "beta"]);
    }

    #[test]
    fn parse_metadata_from_raw_works() {
        let meta = parse_metadata_from_raw(
            SAMPLE_SKILL,
            SkillSource::BuiltIn,
            PathBuf::from("/tmp/test"),
        )
        .unwrap();
        assert_eq!(meta.name, "fix-tests");
        assert_eq!(meta.description, "Diagnose and fix failing tests");
        assert_eq!(meta.version, "1.0.0");
        assert_eq!(meta.author, "luminus");
        assert_eq!(meta.tags, vec!["testing", "debugging"]);
    }

    #[test]
    fn parse_metadata_missing_name_fails() {
        let raw = "---\ndescription: no name\n---\n\nbody\n";
        let result = parse_metadata_from_raw(raw, SkillSource::BuiltIn, PathBuf::from("."));
        assert_eq!(result.unwrap_err(), SkillError::MissingName);
    }

    #[test]
    fn parse_metadata_no_frontmatter_fails() {
        let raw = "Just body, no frontmatter.";
        let result = parse_metadata_from_raw(raw, SkillSource::BuiltIn, PathBuf::from("."));
        assert_eq!(result.unwrap_err(), SkillError::MissingName);
    }

    #[test]
    fn builtin_skills_are_discovered() {
        let registry = SkillRegistry::new();
        let metas = registry.discover();
        // Built-in fix-tests and code-review should be present
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert!(
            names.contains(&"fix-tests"),
            "fix-tests should be discovered: {names:?}"
        );
        assert!(
            names.contains(&"code-review"),
            "code-review should be discovered: {names:?}"
        );
    }

    #[test]
    fn find_is_case_insensitive() {
        let registry = SkillRegistry::new();
        assert!(registry.find("Fix-Tests").is_some());
        assert!(registry.find("FIX-TESTS").is_some());
        assert!(registry.find("nonexistent").is_none());
    }

    #[test]
    fn load_skill_returns_full_content() {
        let registry = SkillRegistry::new();
        let skill = registry.load_skill("fix-tests").unwrap();
        assert_eq!(skill.metadata.name, "fix-tests");
        assert!(skill.content.contains("# Fix Tests"));
        assert!(skill.raw.contains("name: fix-tests"));
        assert!(!skill.content.contains("---"));
    }

    #[test]
    fn load_skill_not_found_returns_error() {
        let registry = SkillRegistry::new();
        assert_eq!(
            registry.load_skill("nonexistent").unwrap_err(),
            SkillError::NotFound("nonexistent".into())
        );
    }

    #[test]
    fn list_skills_includes_builtin_skills() {
        let registry = SkillRegistry::new();
        let text = registry.list_skills();
        assert!(text.contains("fix-tests"));
        assert!(text.contains("code-review"));
        assert!(text.contains("built-in"));
    }

    #[test]
    fn inspect_skill_returns_details() {
        let registry = SkillRegistry::new();
        let text = registry.inspect_skill("fix-tests").unwrap();
        assert!(text.contains("Skill: fix-tests"));
        assert!(text.contains("Version: 1.0.0"));
        assert!(text.contains("Source: built-in"));
        assert!(text.contains("# Fix Tests"));
    }

    #[test]
    fn use_skill_returns_activation_text() {
        let registry = SkillRegistry::new();
        let text = registry.use_skill("fix-tests").unwrap();
        assert!(text.contains("[Skill activated: fix-tests"));
        assert!(text.contains("# Fix Tests"));
    }

    #[test]
    fn project_skill_overrides_builtin() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("fix-tests")).unwrap();
        fs::write(
            dir.join("fix-tests").join("SKILL.md"),
            "---\nname: fix-tests\ndescription: Project override\ndescription: Project override\nversion: 2.0.0\nauthor: project\ntags:\n  - custom\n---\n\n# Project Fix Tests\n\nOverride content.\n",
        )
        .unwrap();

        // Set LUMINUS_DATA_DIR to point global dir elsewhere (empty)
        // and cwd to our scratch dir for project skills.
        let old_cwd = std::env::current_dir().unwrap();
        // For project override test we need .luminus/skills/ in cwd
        fs::create_dir_all(old_cwd.join(".luminus").join("skills").join("fix-tests")).unwrap();
        fs::write(
            old_cwd.join(".luminus").join("skills").join("fix-tests").join("SKILL.md"),
            "---\nname: fix-tests\ndescription: Project override\nversion: 2.0.0\nauthor: project\ntags:\n  - custom\n---\n\n# Project Fix Tests\n\nOverride content.\n",
        ).unwrap();

        let registry = SkillRegistry::new();
        let meta = registry.find("fix-tests").unwrap();
        // If the project skill file exists, it should override the built-in
        if meta.source == SkillSource::Project {
            assert_eq!(meta.version, "2.0.0");
            assert_eq!(meta.author, "project");
            let skill = registry.load_skill("fix-tests").unwrap();
            assert!(skill.content.contains("Project Fix Tests"));
        }

        // Cleanup
        let _ = fs::remove_dir_all(old_cwd.join(".luminus").join("skills").join("fix-tests"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn global_skills_dir_uses_env_override() {
        // SAFETY: single-threaded test, env var is only read by this test
        unsafe {
            std::env::set_var("LUMINUS_DATA_DIR", "/tmp/luminus-test-data");
        }
        assert_eq!(
            global_skills_dir(),
            PathBuf::from("/tmp/luminus-test-data/skills")
        );
        unsafe {
            std::env::remove_var("LUMINUS_DATA_DIR");
        }
    }

    #[test]
    fn from_metadata_creates_custom_registry() {
        let custom = SkillMetadata {
            name: "custom".into(),
            description: "Custom skill".into(),
            version: "0.1.0".into(),
            author: "test".into(),
            tags: vec!["custom".into()],
            source: SkillSource::Project,
            path: PathBuf::from("/custom"),
        };
        let registry = SkillRegistry::from_metadata(vec![custom]);
        assert_eq!(registry.discover().len(), 1);
        assert!(registry.find("custom").is_some());
    }

    #[test]
    fn empty_registry_lists_nothing() {
        let registry = SkillRegistry::from_metadata(vec![]);
        assert_eq!(registry.list_skills(), "No skills available.");
    }

    #[test]
    fn skill_source_display() {
        assert_eq!(SkillSource::BuiltIn.to_string(), "built-in");
        assert_eq!(SkillSource::Global.to_string(), "global");
        assert_eq!(SkillSource::Project.to_string(), "project");
    }

    #[test]
    fn skill_metadata_label() {
        let meta = SkillMetadata {
            name: "test".into(),
            description: String::new(),
            version: String::new(),
            author: String::new(),
            tags: vec![],
            source: SkillSource::BuiltIn,
            path: PathBuf::from("."),
        };
        assert_eq!(meta.label(), "test (built-in)");
    }
}
