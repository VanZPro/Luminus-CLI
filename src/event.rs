use crate::tool_activity::ToolActivity;

/// Events emitted while a provider is processing a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderEvent {
    Started {
        request_id: String,
    },
    Delta {
        request_id: String,
        text: String,
    },
    ToolStarted {
        request_id: String,
        activity: ToolActivity,
    },
    ToolProgress {
        request_id: String,
        activity: ToolActivity,
    },
    ToolCompleted {
        request_id: String,
        activity: ToolActivity,
    },
    ToolFailed {
        request_id: String,
        activity: ToolActivity,
    },
    Completed {
        request_id: String,
    },
    Cancelled {
        request_id: String,
    },
    Failed {
        request_id: String,
        error: String,
    },
}

impl ProviderEvent {
    /// Whether this event permanently ends its request stream.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed { .. } | Self::Cancelled { .. } | Self::Failed { .. }
        )
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::Started { request_id }
            | Self::Delta { request_id, .. }
            | Self::ToolStarted { request_id, .. }
            | Self::ToolProgress { request_id, .. }
            | Self::ToolCompleted { request_id, .. }
            | Self::ToolFailed { request_id, .. }
            | Self::Completed { request_id }
            | Self::Cancelled { request_id }
            | Self::Failed { request_id, .. } => request_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderEvent;

    #[test]
    fn only_outcomes_are_terminal() {
        assert!(
            !ProviderEvent::Started {
                request_id: "r".into()
            }
            .is_terminal()
        );
        assert!(
            !ProviderEvent::Delta {
                request_id: "r".into(),
                text: "chunk".into(),
            }
            .is_terminal()
        );
        assert!(
            ProviderEvent::Completed {
                request_id: "r".into()
            }
            .is_terminal()
        );
        assert!(
            ProviderEvent::Cancelled {
                request_id: "r".into()
            }
            .is_terminal()
        );
        assert!(
            ProviderEvent::Failed {
                request_id: "r".into(),
                error: "nope".into(),
            }
            .is_terminal()
        );
    }
}
