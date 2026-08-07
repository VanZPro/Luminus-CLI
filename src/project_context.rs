use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct ProjectContext {
    pub instructions: Vec<String>,
    pub loaded_paths: Vec<PathBuf>,
}

impl ProjectContext {
    pub fn discover(project_root: impl AsRef<Path>) -> Self {
        let root = project_root.as_ref();
        let mut instructions = Vec::new();
        let mut loaded_paths = Vec::new();
        for relative in ["LUMINUS.md", "AGENTS.md", ".luminus/instructions.md"] {
            let path = root.join(relative);
            if let Ok(content) = fs::read_to_string(&path) {
                instructions.push(format!("# {relative}\n{content}"));
                loaded_paths.push(path);
            }
        }
        Self {
            instructions,
            loaded_paths,
        }
    }

    pub fn formatted_instructions(&self) -> String {
        self.instructions.join("\n\n---\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_project_instructions() {
        let dir =
            std::env::temp_dir().join(format!("luminus-project-context-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("LUMINUS.md"), "Project instruction A").unwrap();
        let ctx = ProjectContext::discover(&dir);
        assert_eq!(ctx.loaded_paths.len(), 1);
        assert!(
            ctx.formatted_instructions()
                .contains("Project instruction A")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
