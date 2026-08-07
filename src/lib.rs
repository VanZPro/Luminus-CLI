pub mod agent;
pub mod app;
pub mod artifact_store;
pub mod command;
pub mod config;
pub mod context;
pub mod diff_history;
pub mod event;
pub mod mcp;
pub mod mission;
pub mod model;
pub mod paths;
pub mod permission_policy;
pub mod project_context;
pub mod providers;
pub mod self_improve;
pub mod session;
pub mod setup;
pub mod skill;
pub mod tool_activity;
pub mod tool_event;
pub mod tool_output;
pub mod tools;

/// Provider abstractions used by the application and the deterministic test provider.
/// Kept in this module so the public `luminus::provider` API is available without
/// requiring a separate provider implementation file.
pub mod provider {
    use std::time::Duration;

    use tokio_util::sync::CancellationToken;

    use crate::event::ProviderEvent;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ProviderCapabilities {
        pub streaming: bool,
        pub cancellation: bool,
    }

    impl ProviderCapabilities {
        pub fn fake() -> Self {
            Self {
                streaming: true,
                cancellation: true,
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ModelInfo {
        pub id: String,
        pub name: String,
        pub context_window: usize,
    }

    impl ModelInfo {
        pub fn fake() -> Self {
            Self {
                id: "fake-model".into(),
                name: "Fake Model".into(),
                context_window: 4096,
            }
        }
    }

    #[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
    pub enum ProviderError {
        #[error("provider request cancelled")]
        Cancelled,
        #[error("provider error: {0}")]
        Message(String),
    }

    pub trait Provider {
        fn model(&self) -> ModelInfo {
            ModelInfo::fake()
        }
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::fake()
        }
        fn stream(
            &self,
            request_id: String,
            prompt: String,
            cancellation: CancellationToken,
        ) -> impl std::future::Future<Output = Vec<ProviderEvent>> + Send;
    }

    /// Optional model-list discovery implemented by providers that can ask the
    /// endpoint for its available models. Offline/fake providers return `None`
    /// and callers fall back to their statically-known catalog.
    pub trait ModelDiscovery {
        type Error;
        fn list_models(
            &self,
        ) -> impl std::future::Future<Output = Result<Vec<String>, Self::Error>> + Send;
    }

    #[derive(Debug, Clone, Copy)]
    pub struct FakeProvider {
        delay: Duration,
    }

    impl FakeProvider {
        pub fn new(delay: Duration) -> Self {
            Self { delay }
        }
        pub fn model(&self) -> ModelInfo {
            ModelInfo::fake()
        }
        pub fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::fake()
        }
    }

    impl Provider for FakeProvider {
        async fn stream(
            &self,
            request_id: String,
            prompt: String,
            cancellation: CancellationToken,
        ) -> Vec<ProviderEvent> {
            let mut events = vec![ProviderEvent::Started {
                request_id: request_id.clone(),
            }];
            if cancellation.is_cancelled() {
                events.push(ProviderEvent::Cancelled { request_id });
                return events;
            }

            if !self.delay.is_zero() {
                tokio::select! {
                    _ = tokio::time::sleep(self.delay) => {}
                    _ = cancellation.cancelled() => {
                        events.push(ProviderEvent::Cancelled { request_id });
                        return events;
                    }
                }
            }
            if cancellation.is_cancelled() {
                events.push(ProviderEvent::Cancelled { request_id });
            } else {
                for text in prompt.split_whitespace() {
                    if cancellation.is_cancelled() {
                        events.push(ProviderEvent::Cancelled { request_id });
                        return events;
                    }
                    events.push(ProviderEvent::Delta {
                        request_id: request_id.clone(),
                        text: text.to_owned(),
                    });
                }
                events.push(ProviderEvent::Completed { request_id });
            }
            events
        }
    }
}

pub mod tui;
