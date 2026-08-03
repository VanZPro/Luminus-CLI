#[path = "../src/providers/openai_compatible.rs"]
mod openai_compatible;
use openai_compatible::*;

#[test]
fn parses_sse_delta_and_done() {
    assert_eq!(
        parse_sse_line(r#"data: {"choices":[{"delta":{"content":"Hi"}}]}"#),
        SseEvent::Delta("Hi".into())
    );
    assert_eq!(parse_sse_line("data: [DONE]"), SseEvent::Done);
    assert_eq!(parse_sse_line(": keep-alive"), SseEvent::Ignore);
}

#[test]
fn parses_completion_and_api_error() {
    assert_eq!(
        parse_chat_completion(r#"{"choices":[{"message":{"content":"answer"}}]}"#).unwrap(),
        "answer"
    );
    assert_eq!(
        parse_chat_completion(r#"{"error":{"message":"bad key","type":"auth","code":"401"}}"#),
        Err(OpenAiError::Api {
            message: "bad key".into(),
            error_type: Some("auth".into()),
            code: Some("401".into())
        })
    );
}

#[test]
fn config_has_secret_safe_debug_and_typed_url() {
    let endpoint = OpenAiCompatibleEndpoint::new("https://example.test/v1/").unwrap();
    assert_eq!(
        endpoint.chat_completions_url(),
        "https://example.test/v1/chat/completions"
    );
    let config = OpenAiCompatibleConfig {
        endpoint,
        api_key: "super-secret".into(),
        model: "gpt-test".into(),
    };
    let debug = format!("{config:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("super-secret"));
}

#[test]
fn request_escapes_json_deterministically() {
    let request = ChatCompletionRequest {
        model: "m".into(),
        stream: true,
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "a\"b\nc".into(),
        }],
    };
    assert_eq!(
        request.json(),
        r#"{"model":"m","messages":[{"role":"user","content":"a\"b\nc"}],"stream":true}"#
    );
}
