//! Stable identity and lifecycle events for tool execution.
//!
//! This module provides a deterministic, dependency-free identity type
//! (`ToolCallId`) and a structured lifecycle event enum
//! (`ToolLifecycleEvent`) for tracking individual tool invocations.
//!
//! It is deliberately self-contained: no UI, async, or provider dependencies.
//! Later phases can wire these events into the existing event bus and TUI.

use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Process-global monotonic counter backing [`ToolCallId`].
///
/// Starting at 1 guarantees the first id is never zero (which we reserve as a
/// sentinel "unset" value in later wiring).
static TOOL_CALL_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Stable, deterministic identifier for a single tool invocation.
///
/// Backed by a process-global `AtomicU64` counter so ids are:
/// - unique within the process,
/// - monotonically increasing,
/// - deterministic (no RNG / UUID dependency),
/// - cheap to copy and compare.
///
/// The internal value is intentionally opaque; callers should compare
/// `ToolCallId` values by equality and use [`as_u64`](Self::as_u64) only when
/// a numeric representation is required (e.g. logging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolCallId {
    value: u64,
}

impl ToolCallId {
    /// Allocate the next unique tool-call identifier.
    pub fn new() -> Self {
        Self {
            value: TOOL_CALL_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Numeric representation of the identifier.
    pub fn as_u64(&self) -> u64 {
        self.value
    }
}

impl Default for ToolCallId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tool:{}", self.value)
    }
}

// ---------------------------------------------------------------------------
// Lifecycle events
// ---------------------------------------------------------------------------

/// Structured lifecycle event for a single tool invocation.
///
/// Every variant carries the [`ToolCallId`] and the tool name so consumers
/// can correlate events without inspecting variant-specific payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolLifecycleEvent {
    /// The tool has been dispatched and is running.
    Started { id: ToolCallId, tool: String },
    /// The tool emitted an intermediate progress update.
    Progress {
        id: ToolCallId,
        tool: String,
        message: String,
    },
    /// The tool finished successfully.
    Completed {
        id: ToolCallId,
        tool: String,
        output: String,
    },
    /// The tool failed with an error.
    Failed {
        id: ToolCallId,
        tool: String,
        error: String,
    },
    /// The tool was cancelled before completion.
    Cancelled { id: ToolCallId, tool: String },
}

impl ToolLifecycleEvent {
    /// The stable identifier of the tool invocation this event belongs to.
    pub fn id(&self) -> ToolCallId {
        *match self {
            Self::Started { id, .. }
            | Self::Progress { id, .. }
            | Self::Completed { id, .. }
            | Self::Failed { id, .. }
            | Self::Cancelled { id, .. } => id,
        }
    }

    /// The name of the tool this event belongs to.
    pub fn tool(&self) -> &str {
        match self {
            Self::Started { tool, .. }
            | Self::Progress { tool, .. }
            | Self::Completed { tool, .. }
            | Self::Failed { tool, .. }
            | Self::Cancelled { tool, .. } => tool,
        }
    }

    /// Whether this event permanently ends the tool's lifecycle stream.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled { .. }
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{ToolCallId, ToolLifecycleEvent};

    #[test]
    fn tool_call_ids_are_unique_and_monotonically_deterministic() {
        let first = ToolCallId::new();
        let second = ToolCallId::new();
        assert_ne!(first, second);
        assert!(second.as_u64() > first.as_u64());
    }

    #[test]
    fn lifecycle_events_preserve_identity_and_payload() {
        let id = ToolCallId::new();
        let events = [
            ToolLifecycleEvent::Started {
                id,
                tool: "read".into(),
            },
            ToolLifecycleEvent::Progress {
                id,
                tool: "read".into(),
                message: "working".into(),
            },
            ToolLifecycleEvent::Completed {
                id,
                tool: "read".into(),
                output: "done".into(),
            },
            ToolLifecycleEvent::Failed {
                id,
                tool: "read".into(),
                error: "oops".into(),
            },
            ToolLifecycleEvent::Cancelled {
                id,
                tool: "read".into(),
            },
        ];
        for event in &events {
            assert_eq!(event.id(), id);
            assert_eq!(event.tool(), "read");
        }
        assert!(matches!(
            &events[1],
            ToolLifecycleEvent::Progress { message, .. } if message == "working"
        ));
        assert!(matches!(
            &events[2],
            ToolLifecycleEvent::Completed { output, .. } if output == "done"
        ));
        assert!(matches!(
            &events[3],
            ToolLifecycleEvent::Failed { error, .. } if error == "oops"
        ));
    }

    #[test]
    fn only_outcome_events_are_terminal() {
        let id = ToolCallId::new();
        assert!(
            !ToolLifecycleEvent::Started {
                id,
                tool: "t".into()
            }
            .is_terminal()
        );
        assert!(
            !ToolLifecycleEvent::Progress {
                id,
                tool: "t".into(),
                message: "m".into()
            }
            .is_terminal()
        );
        assert!(
            ToolLifecycleEvent::Completed {
                id,
                tool: "t".into(),
                output: "o".into()
            }
            .is_terminal()
        );
        assert!(
            ToolLifecycleEvent::Failed {
                id,
                tool: "t".into(),
                error: "e".into()
            }
            .is_terminal()
        );
        assert!(
            ToolLifecycleEvent::Cancelled {
                id,
                tool: "t".into()
            }
            .is_terminal()
        );
    }
}
