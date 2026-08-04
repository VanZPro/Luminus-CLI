//! Provider adapters kept independent from the interactive application.

pub mod openai_compatible;
pub mod openai_runtime;

pub use openai_compatible::{
    ChatCompletionRequest, ChatMessage, ChatResult, ChatRole, OpenAiCompatibleConfig,
    OpenAiCompatibleEndpoint, OpenAiCompatibleProvider, OpenAiError, ReqwestOpenAiTransport,
    SseEvent, parse_chat_completion, parse_sse_line,
};
