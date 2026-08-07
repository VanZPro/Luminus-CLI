//! Small, human-readable persistent conversation sessions.
//!
//! Sessions are stored as JSON files in `<root>/sessions/<name>.json`. The
//! directory root resolves from `LUMINUS_DATA_DIR`, falling back to the
//! platform-appropriate user data directory. Writes are atomic: the session
//! is serialized to a `.tmp` sibling and renamed into place so a crash never
//! leaves a truncated session file.
//!
//! The `messages` field remains the transcript compatibility layer. An
//! optional `events` log records message and tool/approval lifecycle for
//! richer restore later; old files without `events` still load (empty log).

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedMessage {
    pub role: String,
    pub content: String,
}

/// Event-oriented session log entry (messages, tool lifecycle, approvals).
///
/// Serialized with an externally tagged `type` field so the on-disk format is
/// stable and human-readable. Additive: unknown future variants should be
/// introduced carefully; older readers ignore missing fields via defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEvent {
    Message {
        role: String,
        content: String,
    },
    ToolStarted {
        id: String,
        tool: String,
    },
    ToolCompleted {
        id: String,
        tool: String,
        ok: bool,
        summary: String,
    },
    ToolFailed {
        id: String,
        tool: String,
        error: String,
    },
    ToolCancelled {
        id: String,
        tool: String,
        reason: String,
    },
    /// `choice` is a free-form string (e.g. `"allow"`, `"allow_session"`).
    ApprovalResolved {
        tool: String,
        choice: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub created_at: u64,
    pub messages: Vec<SavedMessage>,
    /// Event log for tools/approvals (and dual-written messages). Absent in
    /// legacy JSON; defaults to empty so old files still deserialize.
    #[serde(default)]
    pub events: Vec<SessionEvent>,
}

impl Session {
    pub fn new(name: impl Into<String>, messages: Vec<SavedMessage>) -> Self {
        Self {
            name: name.into(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            messages,
            events: Vec::new(),
        }
    }

    /// Build a session from a messages-only transcript (no events).
    ///
    /// Alias of [`Session::new`] kept for call-site clarity when the caller
    /// intentionally starts with a messages-only snapshot.
    pub fn from_messages(name: impl Into<String>, messages: Vec<SavedMessage>) -> Self {
        Self::new(name, messages)
    }

    /// Append a session event to the event log.
    pub fn append_event(&mut self, event: SessionEvent) {
        self.events.push(event);
    }

    /// Alias for [`Session::append_event`].
    pub fn push_event(&mut self, event: SessionEvent) {
        self.append_event(event);
    }

    /// Borrow the event log.
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    /// If the session has messages but no events (legacy load or messages-only
    /// construction), synthesize a message event stream so consumers can treat
    /// the event log as authoritative without losing transcript content.
    ///
    /// Does nothing when `events` is already non-empty.
    pub fn ensure_events_from_messages(&mut self) {
        if !self.events.is_empty() {
            return;
        }
        self.events = self
            .messages
            .iter()
            .map(|m| SessionEvent::Message {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();
    }

    pub fn save(&self, root: impl AsRef<Path>) -> io::Result<PathBuf> {
        let directory = root.as_ref().join("sessions");
        fs::create_dir_all(&directory)?;
        let path = directory.join(format!("{}.json", safe_name(&self.name)));
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }

    pub fn load(root: impl AsRef<Path>, name: &str) -> io::Result<Self> {
        let path = root
            .as_ref()
            .join("sessions")
            .join(format!("{}.json", safe_name(name)));
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(io::Error::other)
    }

    pub fn list(root: impl AsRef<Path>) -> io::Result<Vec<String>> {
        let directory = root.as_ref().join("sessions");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut names: Vec<String> = fs::read_dir(directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .path()
                    .file_stem()
                    .map(|x| x.to_string_lossy().into_owned())
            })
            .collect();
        names.sort();
        Ok(names)
    }

    pub fn delete(root: impl AsRef<Path>, name: &str) -> io::Result<bool> {
        let path = root
            .as_ref()
            .join("sessions")
            .join(format!("{}.json", safe_name(name)));
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
}

/// Resolves the persistent data directory for the current platform.
pub fn default_root() -> PathBuf {
    if let Some(path) = std::env::var_os("LUMINUS_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("luminus");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".local/share/luminus");
    }
    PathBuf::from(".luminus")
}

fn safe_name(name: &str) -> String {
    let normalized: String = name
        .trim()
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
        "default".into()
    } else {
        normalized.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "luminus-session-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn save_load_list_delete_round_trip_is_deterministic() {
        let root = scratch_root();
        let _ = fs::remove_dir_all(&root);

        let session = Session::new(
            "demo session",
            vec![SavedMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
        );
        let saved_path = session.save(&root).unwrap();
        assert!(saved_path.ends_with("sessions/demo_session.json"));

        let names = Session::list(&root).unwrap();
        assert_eq!(names, ["demo_session"]);

        let loaded = Session::load(&root, "demo session").unwrap();
        assert_eq!(loaded.name, "demo session");
        assert_eq!(loaded.messages, session.messages);
        assert!(loaded.events.is_empty());

        assert!(Session::delete(&root, "demo session").unwrap());
        assert_eq!(Session::list(&root).unwrap(), Vec::<String>::new());
        assert!(!Session::delete(&root, "demo session").unwrap());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safe_name_normalizes_unfriendly_characters() {
        assert_eq!(safe_name("hello world"), "hello_world");
        assert_eq!(safe_name("../../etc"), "______etc");
        assert_eq!(safe_name("  "), "default");
        let long = "a".repeat(120);
        assert_eq!(safe_name(&long).len(), 80);
    }

    #[test]
    fn default_root_resolves_when_env_missing() {
        // SAFETY: this test mutates a process-wide environment variable. The
        // test binary is single-threaded for env access and we restore the
        // variable's absence at the end of the test.
        unsafe {
            std::env::remove_var("LUMINUS_DATA_DIR");
        }
        let root = default_root();
        assert!(!root.as_os_str().is_empty());
    }

    #[test]
    fn roundtrip_with_events_preserves_tool_and_approval_log() {
        let root = scratch_root();
        let _ = fs::remove_dir_all(&root);

        let mut session = Session::from_messages(
            "eventful",
            vec![SavedMessage {
                role: "user".into(),
                content: "run shell".into(),
            }],
        );
        session.push_event(SessionEvent::Message {
            role: "user".into(),
            content: "run shell".into(),
        });
        session.append_event(SessionEvent::ToolStarted {
            id: "tool:1".into(),
            tool: "shell".into(),
        });
        session.append_event(SessionEvent::ApprovalResolved {
            tool: "shell".into(),
            choice: "allow_session".into(),
        });
        session.append_event(SessionEvent::ToolCompleted {
            id: "tool:1".into(),
            tool: "shell".into(),
            ok: true,
            summary: "ok".into(),
        });
        session.append_event(SessionEvent::ToolFailed {
            id: "tool:2".into(),
            tool: "write".into(),
            error: "denied".into(),
        });
        session.append_event(SessionEvent::ToolCancelled {
            id: "tool:3".into(),
            tool: "bash".into(),
            reason: "user_abort".into(),
        });

        session.save(&root).unwrap();
        let loaded = Session::load(&root, "eventful").unwrap();
        assert_eq!(loaded.messages, session.messages);
        assert_eq!(loaded.events().len(), 6);
        assert_eq!(loaded.events(), session.events());
        assert!(matches!(
            &loaded.events()[0],
            SessionEvent::Message { role, content }
                if role == "user" && content == "run shell"
        ));
        assert!(matches!(
            &loaded.events()[2],
            SessionEvent::ApprovalResolved { tool, choice }
                if tool == "shell" && choice == "allow_session"
        ));
        assert!(matches!(
            &loaded.events()[3],
            SessionEvent::ToolCompleted { ok: true, summary, .. }
                if summary == "ok"
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_legacy_json_without_events_field_defaults_empty() {
        let root = scratch_root();
        let _ = fs::remove_dir_all(&root);
        let directory = root.join("sessions");
        fs::create_dir_all(&directory).unwrap();

        // Pre-events on-disk shape: name, created_at, messages only.
        let legacy = r#"{
  "name": "legacy",
  "created_at": 1700000000,
  "messages": [
    { "role": "user", "content": "hi" },
    { "role": "assistant", "content": "hello" }
  ]
}"#;
        fs::write(directory.join("legacy.json"), legacy).unwrap();

        let loaded = Session::load(&root, "legacy").unwrap();
        assert_eq!(loaded.name, "legacy");
        assert_eq!(loaded.created_at, 1_700_000_000);
        assert_eq!(loaded.messages.len(), 2);
        assert!(loaded.events.is_empty());
        assert!(loaded.events().is_empty());

        // Optional conversion for messages-only loads.
        let mut converted = loaded.clone();
        converted.ensure_events_from_messages();
        assert_eq!(converted.events().len(), 2);
        assert!(matches!(
            &converted.events()[1],
            SessionEvent::Message { role, content }
                if role == "assistant" && content == "hello"
        ));
        // Idempotent: does not re-synthesize when events already present.
        converted.ensure_events_from_messages();
        assert_eq!(converted.events().len(), 2);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_event_serde_uses_type_tag() {
        let event = SessionEvent::ToolStarted {
            id: "1".into(),
            tool: "read".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "tool_started");
        assert_eq!(json["id"], "1");
        assert_eq!(json["tool"], "read");

        let round: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(round, event);
    }
}
