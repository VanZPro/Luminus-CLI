//! Runtime glue: adapts the OpenAI-compatible chat adapter to the application's
//! [`crate::provider::Provider`] trait and selects between the fake (offline)
//! and the real HTTP provider at runtime.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::event::ProviderEvent;
use crate::provider::{ModelDiscovery, ModelInfo, Provider, ProviderCapabilities};

use super::openai_compatible::{
    ChatCompletionRequest, ChatMessage, ChatResult, ChatRole, OpenAiCompatibleConfig,
    OpenAiCompatibleEndpoint, OpenAiCompatibleProvider, OpenAiError, OpenAiTransport,
    ReqwestOpenAiTransport, SseEvent,
};

/// Model used when the environment does not configure one.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// OpenAI-compatible provider implementing the application `Provider` trait.
///
/// The transport is generic so tests can inject a deterministic stub; the
/// default is a real `reqwest` HTTP client.
#[derive(Debug, Clone)]
pub struct OpenAiProvider<T: OpenAiTransport = ReqwestOpenAiTransport> {
    inner: OpenAiCompatibleProvider<T>,
    model_id: String,
}

impl<T: OpenAiTransport> OpenAiProvider<T> {
    /// Builds a provider around an arbitrary transport (used by tests).
    pub fn with_transport(config: OpenAiCompatibleConfig, transport: T) -> Self {
        let model_id = config.model.clone();
        Self {
            inner: OpenAiCompatibleProvider::new(config, transport),
            model_id,
        }
    }
}

impl OpenAiProvider<ReqwestOpenAiTransport> {
    /// Builds a real-HTTP provider.
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, OpenAiError> {
        Ok(Self::with_transport(config, ReqwestOpenAiTransport::new()?))
    }

    /// Loads configuration from the environment when an API key is present.
    ///
    /// Returns `None` when no key is configured (the caller should fall back
    /// to the fake provider) and `Some(Err(..))` when a key exists but the
    /// configuration is invalid (e.g. a non-http base URL).
    pub fn from_env() -> Option<Result<Self, OpenAiError>> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("LUMINUS_OPENAI_API_KEY"))
            .ok()?;
        if api_key.trim().is_empty() {
            return None;
        }
        let base_url = std::env::var("OPENAI_BASE_URL")
            .or_else(|_| std::env::var("LUMINUS_OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned());
        let model = std::env::var("OPENAI_MODEL")
            .or_else(|_| std::env::var("LUMINUS_OPENAI_MODEL"))
            .unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        let endpoint = match OpenAiCompatibleEndpoint::new(base_url) {
            Ok(endpoint) => endpoint,
            Err(error) => return Some(Err(error)),
        };
        Some(Self::new(OpenAiCompatibleConfig {
            endpoint,
            api_key: api_key.to_owned(),
            model,
        }))
    }
}

impl<T: OpenAiTransport> ModelDiscovery for OpenAiProvider<T> {
    type Error = OpenAiError;
    async fn list_models(&self) -> Result<Vec<String>, Self::Error> {
        self.inner.list_models().await.map(|list| list.ids)
    }
}

impl<T: OpenAiTransport> Provider for OpenAiProvider<T> {
    fn model(&self) -> ModelInfo {
        ModelInfo {
            id: self.model_id.clone(),
            name: self.model_id.clone(),
            context_window: 128_000,
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            cancellation: true,
        }
    }

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

        let request = ChatCompletionRequest {
            model: self.model_id.clone(),
            stream: true,
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: prompt,
            }],
        };

        match self.inner.chat(request).await {
            Ok(ChatResult::Complete(text)) => {
                events.push(ProviderEvent::Delta {
                    request_id: request_id.clone(),
                    text,
                });
                events.push(ProviderEvent::Completed { request_id });
            }
            Ok(ChatResult::Stream(chunks)) => {
                for chunk in chunks {
                    if cancellation.is_cancelled() {
                        events.push(ProviderEvent::Cancelled { request_id });
                        return events;
                    }
                    match chunk {
                        SseEvent::Delta(text) => events.push(ProviderEvent::Delta {
                            request_id: request_id.clone(),
                            text,
                        }),
                        SseEvent::Done => {
                            events.push(ProviderEvent::Completed { request_id });
                            return events;
                        }
                        SseEvent::Error(error) => {
                            events.push(ProviderEvent::Failed {
                                request_id,
                                error: error.to_string(),
                            });
                            return events;
                        }
                        SseEvent::Ignore => {}
                    }
                }
                events.push(ProviderEvent::Completed { request_id });
            }
            Err(error) => {
                events.push(ProviderEvent::Failed {
                    request_id,
                    error: error.to_string(),
                });
            }
        }
        events
    }
}

/// Runtime-selected provider: fake (offline) or real OpenAI-compatible.
#[derive(Debug, Clone)]
pub enum RuntimeProvider {
    Fake(crate::provider::FakeProvider),
    OpenAi(OpenAiProvider<ReqwestOpenAiTransport>),
}

impl RuntimeProvider {
    /// Builds the runtime provider: real OpenAI when the environment has an
    /// API key, otherwise the deterministic fake provider.
    pub fn from_env_or_fake(delay: Duration) -> Self {
        match OpenAiProvider::from_env() {
            Some(Ok(provider)) => Self::OpenAi(provider),
            _ => Self::Fake(crate::provider::FakeProvider::new(delay)),
        }
    }

    /// Whether the runtime is the real OpenAI-compatible provider.
    pub fn is_openai(&self) -> bool {
        matches!(self, Self::OpenAi(_))
    }
}

impl ModelDiscovery for RuntimeProvider {
    type Error = OpenAiError;

    async fn list_models(&self) -> Result<Vec<String>, Self::Error> {
        match self {
            Self::Fake(_) => Ok(vec![crate::provider::ModelInfo::fake().id]),
            Self::OpenAi(provider) => provider.list_models().await,
        }
    }
}

impl Provider for RuntimeProvider {
    fn model(&self) -> ModelInfo {
        match self {
            Self::Fake(provider) => provider.model(),
            Self::OpenAi(provider) => provider.model(),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        match self {
            Self::Fake(provider) => provider.capabilities(),
            Self::OpenAi(provider) => provider.capabilities(),
        }
    }

    async fn stream(
        &self,
        request_id: String,
        prompt: String,
        cancellation: CancellationToken,
    ) -> Vec<ProviderEvent> {
        match self {
            Self::Fake(provider) => provider.stream(request_id, prompt, cancellation).await,
            Self::OpenAi(provider) => provider.stream(request_id, prompt, cancellation).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(model: &str) -> OpenAiCompatibleConfig {
        OpenAiCompatibleConfig {
            endpoint: OpenAiCompatibleEndpoint::new("https://example.test/v1/").unwrap(),
            api_key: "stub".into(),
            model: model.into(),
        }
    }

    /// Deterministic transport replaying a canned HTTP response.
    #[derive(Debug, Clone)]
    struct StubTransport {
        status: u16,
        body: String,
    }

    impl OpenAiTransport for StubTransport {
        fn send(
            &self,
            _request: super::super::openai_compatible::OpenAiRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            super::super::openai_compatible::OpenAiResponse,
                            OpenAiError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            let status = self.status;
            let body = self.body.clone();
            Box::pin(
                async move { Ok(super::super::openai_compatible::OpenAiResponse { status, body }) },
            )
        }
    }

    fn kind(event: &ProviderEvent) -> &'static str {
        match event {
            ProviderEvent::Started { .. } => "started",
            ProviderEvent::Delta { .. } => "delta",
            ProviderEvent::ToolStarted { .. } => "tool_started",
            ProviderEvent::ToolProgress { .. } => "tool_progress",
            ProviderEvent::ToolCompleted { .. } => "tool_completed",
            ProviderEvent::ToolFailed { .. } => "tool_failed",
            ProviderEvent::Completed { .. } => "completed",
            ProviderEvent::Cancelled { .. } => "cancelled",
            ProviderEvent::Failed { .. } => "failed",
        }
    }

    fn kinds(events: &[ProviderEvent]) -> Vec<&'static str> {
        events.iter().map(kind).collect()
    }

    #[tokio::test]
    async fn maps_streamed_deltas_to_provider_events() {
        let transport = StubTransport {
            status: 200,
            body: [
                r#"data: {"choices":[{"delta":{"content":"Hello"}}]}"#,
                "",
                r#"data: {"choices":[{"delta":{"content":" world"}}]}"#,
                "",
                "data: [DONE]",
                "",
            ]
            .join("\n"),
        };
        let provider = OpenAiProvider::with_transport(config("stub-model"), transport);
        let events = provider
            .stream("r1".into(), "hi".into(), CancellationToken::new())
            .await;

        assert_eq!(kinds(&events), ["started", "delta", "delta", "completed"]);
        let ProviderEvent::Delta { text, .. } = &events[1] else {
            panic!("expected delta");
        };
        assert_eq!(text, "Hello");
        let ProviderEvent::Delta { text, .. } = &events[2] else {
            panic!("expected delta");
        };
        assert_eq!(text, " world");
    }

    #[tokio::test]
    async fn non_streaming_completion_becomes_one_delta() {
        let transport = StubTransport {
            status: 200,
            body: r#"{"choices":[{"message":{"content":"full answer"}}]}"#.into(),
        };
        let provider = OpenAiProvider::with_transport(config("stub-model"), transport);
        let events = provider
            .stream("r1".into(), "hi".into(), CancellationToken::new())
            .await;

        assert_eq!(kinds(&events), ["started", "delta", "completed"]);
    }

    #[tokio::test]
    async fn cancellation_before_request_emits_cancelled() {
        let transport = StubTransport {
            status: 200,
            body: "data: [DONE]".into(),
        };
        let provider = OpenAiProvider::with_transport(config("stub-model"), transport);
        let token = CancellationToken::new();
        token.cancel();
        let events = provider.stream("r1".into(), "hi".into(), token).await;

        assert_eq!(kinds(&events), ["started", "cancelled"]);
    }

    #[tokio::test]
    async fn api_error_becomes_failed() {
        let transport = StubTransport {
            status: 401,
            body: r#"{"error":{"message":"bad key","type":"auth","code":"401"}}"#.into(),
        };
        let provider = OpenAiProvider::with_transport(config("stub-model"), transport);
        let events = provider
            .stream("r1".into(), "hi".into(), CancellationToken::new())
            .await;

        assert_eq!(kinds(&events), ["started", "failed"]);
        let ProviderEvent::Failed { error, .. } = &events[1] else {
            panic!("expected failed");
        };
        assert!(error.contains("bad key"), "error was: {error}");
    }

    #[test]
    fn env_configuration_is_deterministic() {
        // All environment scenarios run sequentially in one test so parallel
        // tests cannot race on the same variables.
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("LUMINUS_OPENAI_API_KEY");
            std::env::remove_var("OPENAI_BASE_URL");
            std::env::remove_var("OPENAI_MODEL");
        }

        // No key configured -> fall back to the fake provider.
        assert!(!RuntimeProvider::from_env_or_fake(Duration::from_millis(1)).is_openai());

        // Valid key + base + model -> real OpenAI provider with the model id.
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test-key");
            std::env::set_var("OPENAI_BASE_URL", "https://example.test/v1/");
            std::env::set_var("OPENAI_MODEL", "stub-model");
        }
        let provider = RuntimeProvider::from_env_or_fake(Duration::from_millis(1));
        assert!(provider.is_openai());
        assert_eq!(provider.model().id, "stub-model");

        // Key present but invalid base URL -> configuration error, not fallback.
        unsafe {
            std::env::set_var("OPENAI_BASE_URL", "nonsense");
        }
        assert!(matches!(
            OpenAiProvider::from_env(),
            Some(Err(OpenAiError::InvalidEndpoint))
        ));

        // Blank key counts as absent.
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "   ");
            std::env::set_var("OPENAI_BASE_URL", "https://example.test/v1/");
        }
        assert!(OpenAiProvider::from_env().is_none());

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("LUMINUS_OPENAI_API_KEY");
            std::env::remove_var("OPENAI_BASE_URL");
            std::env::remove_var("OPENAI_MODEL");
        }
    }
}
