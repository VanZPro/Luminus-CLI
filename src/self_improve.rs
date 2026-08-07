use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedSkillDraft {
    pub name: String,
    pub description: String,
    pub body: String,
}

impl LearnedSkillDraft {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            body: body.into(),
        }
    }

    pub fn to_skill_md(&self) -> String {
        format!(
            "---\nname: {}\ndescription: {}\nversion: 0.1.0\nauthor: luminus-self-improve\ntags:\n  - learned\n  - workflow\n---\n\n{}\n",
            self.name,
            self.description,
            self.body.trim()
        )
    }
}

#[derive(Debug, Clone)]
pub struct SelfImproveStore {
    root: PathBuf,
}

impl SelfImproveStore {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join(".luminus").join("skills"),
        }
    }

    pub fn save_skill(&self, draft: &LearnedSkillDraft) -> std::io::Result<PathBuf> {
        let skill_dir = self.root.join(&draft.name);
        fs::create_dir_all(&skill_dir)?;
        let path = skill_dir.join("SKILL.md");
        fs::write(&path, draft.to_skill_md())?;
        Ok(path)
    }

    pub fn draft_from_summary(name: &str, task: &str, lessons: &[String]) -> LearnedSkillDraft {
        let mut body = format!(
            "# {}\n\n## When to use\n{}\n\n## Steps\n",
            name,
            task.trim()
        );
        for (idx, lesson) in lessons.iter().enumerate() {
            body.push_str(&format!("{}. {}\n", idx + 1, lesson.trim()));
        }
        LearnedSkillDraft::new(name, format!("Learned workflow for {task}"), body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_writes_skill_md() {
        let dir = std::env::temp_dir().join(format!("luminus-self-improve-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = SelfImproveStore::new(&dir);
        let draft = SelfImproveStore::draft_from_summary(
            "repeat-task",
            "repeat task",
            &["Do A".into(), "Verify B".into()],
        );
        let path = store.save_skill(&draft).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("name: repeat-task"));
        assert!(text.contains("Verify B"));
        let _ = fs::remove_dir_all(&dir);
    }
}
