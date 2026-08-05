#![allow(dead_code)]

use std::{fmt, future::Future, pin::Pin, str::FromStr};

#[derive(Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleConfig {
    pub endpoint: OpenAiCompatibleEndpoint,
    pub api_key: String,
    pub model: String,
}
impl fmt::Debug for OpenAiCompatibleConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatibleConfig")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleEndpoint(String);
impl OpenAiCompatibleEndpoint {
    pub fn new(value: impl Into<String>) -> Result<Self, OpenAiError> {
        let s = value.into();
        if s.starts_with("http://") || s.starts_with("https://") {
            Ok(Self(s.trim_end_matches('/').to_owned()))
        } else {
            Err(OpenAiError::InvalidEndpoint)
        }
    }
    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.0)
    }

    pub fn models_url(&self) -> String {
        format!("{}/models", self.0)
    }
}
impl fmt::Debug for OpenAiCompatibleEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("OpenAiCompatibleEndpoint")
            .field(&self.0)
            .finish()
    }
}
impl FromStr for OpenAiCompatibleEndpoint {
    type Err = OpenAiError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ChatRole {
    System,
    User,
    Assistant,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub struct OpenAiRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}
impl fmt::Debug for OpenAiRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiResponse {
    pub status: u16,
    pub body: String,
}

/// Extracts model identifiers from the OpenAI `/models` response.
///
/// The parser intentionally accepts the common JSON shape without requiring a
/// full schema: `{ "data": [{ "id": "model-name" }] }`.
pub fn parse_model_ids(body: &str) -> Result<Vec<String>, OpenAiError> {
    if body.contains(r#""error""#) {
        return Err(parse_error(body));
    }
    let mut ids = Vec::new();
    let mut cursor = body;
    while let Some(position) = cursor.find(r#""id""#) {
        let rest = &cursor[position + 4..];
        let Some(colon) = rest.find(':') else { break };
        let value = rest[colon + 1..].trim_start();
        let Some(value) = value.strip_prefix('"') else {
            cursor = &rest[colon + 1..];
            continue;
        };
        let mut id = String::new();
        let mut escaped = false;
        for ch in value.chars() {
            if escaped {
                id.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            } else {
                id.push(ch);
            }
        }
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
        cursor = &value[value.find('"').unwrap_or(value.len())..];
        if cursor.is_empty() {
            break;
        }
    }
    if ids.is_empty() {
        Err(OpenAiError::InvalidJson)
    } else {
        Ok(ids)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelList {
    pub ids: Vec<String>,
}

impl ModelList {
    pub fn from_response(response: &OpenAiResponse) -> Result<Self, OpenAiError> {
        if response.status >= 400 {
            return Err(parse_error(&response.body));
        }
        Ok(Self {
            ids: parse_model_ids(&response.body)?,
        })
    }
}

pub trait OpenAiTransport: Send + Sync {
    fn send(
        &self,
        request: OpenAiRequest,
    ) -> Pin<Box<dyn Future<Output = Result<OpenAiResponse, OpenAiError>> + Send + '_>>;
}

/// Reqwest-backed HTTP transport. Credentials are not retained by the transport.
#[derive(Clone, Default)]
pub struct ReqwestOpenAiTransport {
    client: reqwest::Client,
}

impl ReqwestOpenAiTransport {
    pub fn new() -> Result<Self, OpenAiError> {
        reqwest::Client::builder()
            .build()
            .map(|client| Self { client })
            .map_err(|e| OpenAiError::Transport(e.to_string()))
    }
}

impl fmt::Debug for ReqwestOpenAiTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReqwestOpenAiTransport").finish()
    }
}

impl OpenAiTransport for ReqwestOpenAiTransport {
    fn send(
        &self,
        request: OpenAiRequest,
    ) -> Pin<Box<dyn Future<Output = Result<OpenAiResponse, OpenAiError>> + Send + '_>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|e| OpenAiError::Transport(e.to_string()))?;
            let mut builder = self.client.request(method, &request.url);
            for (name, value) in request.headers {
                builder = builder.header(name, value);
            }
            let response = builder
                .body(request.body)
                .send()
                .await
                .map_err(|e| OpenAiError::Transport(e.to_string()))?;
            let status = response.status().as_u16();
            let body = response
                .text()
                .await
                .map_err(|e| OpenAiError::Transport(e.to_string()))?;
            Ok(OpenAiResponse { status, body })
        })
    }
}

impl OpenAiError {
    // Kept private to the public enum's existing parser-facing variants.
}
#[derive(Clone)]
pub struct OpenAiCompatibleProvider<T> {
    pub config: OpenAiCompatibleConfig,
    pub transport: T,
}
impl<T: fmt::Debug> fmt::Debug for OpenAiCompatibleProvider<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiCompatibleProvider")
            .field("config", &self.config)
            .field("transport", &self.transport)
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatResult {
    Complete(String),
    Stream(Vec<SseEvent>),
}
impl<T: OpenAiTransport> OpenAiCompatibleProvider<T> {
    pub fn new(config: OpenAiCompatibleConfig, transport: T) -> Self {
        Self { config, transport }
    }

    pub async fn list_models(&self) -> Result<ModelList, OpenAiError> {
        let response = self
            .transport
            .send(OpenAiRequest {
                method: "GET".into(),
                url: self.config.endpoint.models_url(),
                headers: vec![(
                    "authorization".into(),
                    format!("Bearer {}", self.config.api_key),
                )],
                body: String::new(),
            })
            .await?;
        ModelList::from_response(&response)
    }

    pub async fn chat(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<ChatResult, OpenAiError> {
        request.model = if request.model.is_empty() {
            self.config.model.clone()
        } else {
            request.model
        };
        let response = self
            .transport
            .send(OpenAiRequest {
                method: "POST".into(),
                url: self.config.endpoint.chat_completions_url(),
                headers: vec![
                    (
                        "authorization".into(),
                        format!("Bearer {}", self.config.api_key),
                    ),
                    ("content-type".into(), "application/json".into()),
                ],
                body: request.json(),
            })
            .await?;
        if response.status >= 400 {
            return Err(parse_error(&response.body));
        }
        if request.stream {
            // Some compatible endpoints ignore `stream=true` and return a
            // regular JSON completion. Accept that response instead of
            // silently producing an empty stream.
            if response.body.trim_start().starts_with('{') {
                Ok(ChatResult::Complete(parse_chat_completion(&response.body)?))
            } else {
                Ok(ChatResult::Stream(
                    response.body.lines().map(parse_sse_line).collect(),
                ))
            }
        } else {
            Ok(ChatResult::Complete(parse_chat_completion(&response.body)?))
        }
    }
}
impl ChatCompletionRequest {
    pub fn json(&self) -> String {
        let msgs = self
            .messages
            .iter()
            .map(|m| {
                format!(
                    r#"{{"role":"{}","content":"{}"}}"#,
                    match m.role {
                        ChatRole::System => "system",
                        ChatRole::User => "user",
                        ChatRole::Assistant => "assistant",
                    },
                    escape(&m.content)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"model":"{}","messages":[{}],"stream":{}}}"#,
            escape(&self.model),
            msgs,
            self.stream
        )
    }
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Delta(String),
    Done,
    Error(OpenAiError),
    Ignore,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAiError {
    InvalidEndpoint,
    InvalidJson,
    Transport(String),
    Api {
        message: String,
        error_type: Option<String>,
        code: Option<String>,
    },
}
impl fmt::Display for OpenAiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint => write!(f, "endpoint must be http(s)"),
            Self::InvalidJson => write!(f, "invalid JSON"),
            Self::Transport(message) => write!(f, "HTTP transport error: {message}"),
            Self::Api { message, .. } => write!(f, "OpenAI API error: {message}"),
        }
    }
}
impl std::error::Error for OpenAiError {}

pub fn parse_sse_line(line: &str) -> SseEvent {
    let data = line.trim().strip_prefix("data:").map(str::trim);
    let Some(data) = data else {
        return SseEvent::Ignore;
    };
    if data == "[DONE]" {
        return SseEvent::Done;
    }
    if data.is_empty() {
        return SseEvent::Ignore;
    }
    match extract_string(data, "content") {
        Some(v) => SseEvent::Delta(v),
        None => {
            if data.contains(r#""error""#) {
                SseEvent::Error(parse_error(data))
            } else {
                SseEvent::Ignore
            }
        }
    }
}
pub fn parse_chat_completion(body: &str) -> Result<String, OpenAiError> {
    extract_string(body, "content").ok_or_else(|| {
        if body.contains(r#""error""#) {
            parse_error(body)
        } else {
            OpenAiError::InvalidJson
        }
    })
}
fn parse_error(s: &str) -> OpenAiError {
    OpenAiError::Api {
        message: extract_string(s, "message").unwrap_or_else(|| "unknown provider error".into()),
        error_type: extract_string(s, "type"),
        code: extract_string(s, "code"),
    }
}
fn extract_string(s: &str, key: &str) -> Option<String> {
    let needle = format!(r#""{}""#, key);
    let p = s.find(&needle)?;
    let rest = &s[p + needle.len()..];
    let colon = rest.find(':')?;
    let v = rest[colon + 1..].trim_start();
    if !v.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut esc = false;
    for c in v[1..].chars() {
        if esc {
            out.push(match c {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                x => x,
            });
            esc = false
        } else if c == '\\' {
            esc = true
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c)
        }
    }
    None
}
