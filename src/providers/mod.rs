//! Provider adapters kept independent from the interactive application.

pub mod openai_compatible;

pub use openai_compatible::{
    ChatCompletionRequest, ChatMessage, ChatRole, OpenAiCompatibleConfig, OpenAiCompatibleEndpoint,
    OpenAiError, ReqwestOpenAiTransport, SseEvent, parse_chat_completion, parse_sse_line,
};
