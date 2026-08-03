//! Structured tool activity events and deterministic TUI-card formatting.
//!
//! This module deliberately has no UI or async dependencies.  The parent application can
//! translate these values into its event bus and render `ToolActivity::card()` as needed.

use std::fmt;
use std::time::Duration;

/// The lifecycle state represented by an activity event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Started,
    InProgress,
    Completed,
    Failed,
}

impl fmt::Display for ToolStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Started => "started",
            Self::InProgress => "progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
        })
    }
}

/// Common metadata shared by all tool activity events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivityMeta {
    pub tool: String,
    pub status: ToolStatus,
    pub duration: Option<Duration>,
}

impl ToolActivityMeta {
    pub fn new(tool: impl Into<String>, status: ToolStatus) -> Self {
        Self {
            tool: tool.into(),
            status,
            duration: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }
}

/// A typed lifecycle event emitted by a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolActivity {
    Started {
        meta: ToolActivityMeta,
    },
    Progress {
        meta: ToolActivityMeta,
        message: String,
    },
    Completed {
        meta: ToolActivityMeta,
        output: String,
    },
    Failed {
        meta: ToolActivityMeta,
        error: String,
    },
}

impl ToolActivity {
    pub fn started(tool: impl Into<String>) -> Self {
        Self::Started {
            meta: ToolActivityMeta::new(tool, ToolStatus::Started),
        }
    }

    pub fn progress(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Progress {
            meta: ToolActivityMeta::new(tool, ToolStatus::InProgress),
            message: message.into(),
        }
    }

    pub fn completed(tool: impl Into<String>, output: impl Into<String>) -> Self {
        Self::Completed {
            meta: ToolActivityMeta::new(tool, ToolStatus::Completed),
            output: output.into(),
        }
    }

    pub fn failed(tool: impl Into<String>, error: impl Into<String>) -> Self {
        Self::Failed {
            meta: ToolActivityMeta::new(tool, ToolStatus::Failed),
            error: error.into(),
        }
    }

    pub fn meta(&self) -> &ToolActivityMeta {
        match self {
            Self::Started { meta }
            | Self::Progress { meta, .. }
            | Self::Completed { meta, .. }
            | Self::Failed { meta, .. } => meta,
        }
    }

    pub fn with_duration(self, duration: Duration) -> Self {
        let mut event = self;
        match &mut event {
            Self::Started { meta }
            | Self::Progress { meta, .. }
            | Self::Completed { meta, .. }
            | Self::Failed { meta, .. } => meta.duration = Some(duration),
        }
        event
    }

    /// Stable, line-oriented representation suitable for a TUI card or snapshot test.
    pub fn card(&self) -> String {
        let meta = self.meta();
        let detail = match self {
            Self::Started { .. } => None,
            Self::Progress { message, .. } => Some(message),
            Self::Completed { output, .. } => Some(output),
            Self::Failed { error, .. } => Some(error),
        };
        let mut result = format!("[{}] {}", meta.status, meta.tool);
        if let Some(duration) = meta.duration {
            result.push_str(&format!(" ({}ms)", duration.as_millis()));
        }
        if let Some(detail) = detail {
            result.push('\n');
            result.push_str(&truncate(detail, 500));
        }
        result
    }
}

/// Truncate by Unicode scalar values (never split UTF-8), appending an ellipsis when needed.
pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_owned();
    }
    let mut result: String = text.chars().take(max_chars - 1).collect();
    result.push('…');
    result
}

pub const CARD_DETAIL_LIMIT: usize = 500;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_truncation_is_safe() {
        assert_eq!(truncate("😀abcdef", 4), "😀ab…");
    }
}
