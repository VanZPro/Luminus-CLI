use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    #[default]
    Queued,
    Planning,
    Running,
    WaitingForApproval,
    Paused,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub status: MissionStatus,
    pub created_at: u64,
}

impl Mission {
    pub fn new(id: impl Into<String>, goal: impl Into<String>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let goal_str = goal.into();
        let title = if goal_str.len() > 30 {
            format!("{}...", &goal_str[0..27])
        } else {
            goal_str.clone()
        };

        Self {
            id: id.into(),
            title,
            goal: goal_str,
            status: MissionStatus::Queued,
            created_at: now,
        }
    }
}

pub struct MissionStore {
    root: PathBuf,
}

impl MissionStore {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join(".luminus").join("missions"),
        }
    }

    pub fn save(&self, mission: &Mission) -> std::io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let path = self.root.join(format!("{}.json", mission.id));
        let json = serde_json::to_string_pretty(mission)?;
        fs::write(path, json)
    }

    pub fn load(&self, id: &str) -> Option<Mission> {
        let path = self.root.join(format!("{}.json", id));
        let json = fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }

    pub fn list(&self) -> Vec<Mission> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_mission() {
        let dir = std::env::temp_dir().join(format!("luminus-mission-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = MissionStore::new(&dir);

        let mut mission = Mission::new("m-1", "Build the database schema");
        mission.status = MissionStatus::Running;
        store.save(&mission).unwrap();

        let loaded = store.load("m-1").unwrap();
        assert_eq!(loaded.title, "Build the database schema");
        assert_eq!(loaded.status, MissionStatus::Running);

        let _ = fs::remove_dir_all(&dir);
    }
}
