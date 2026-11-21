//! A caller's own provider and hand-written tool, built from outside the
//! crate through the public constructors only.
#![cfg(feature = "agent")]

use async_trait::async_trait;
use llm_provider::agent::{CallContext, Tool, ToolOutput};
use llm_provider::provider::{Content, MessageRole, ModelSelection};
use llm_provider::{
    CompletionRequest, EventStream, LlmProvider, Message, ProviderInfo, StreamEvent, ToolDefinition,
};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

struct Echo;

#[async_trait]
impl LlmProvider for Echo {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        let text = request
            .messages
            .last()
            .map(Message::text)
            .unwrap_or_default();
        Box::pin(tokio_stream::iter([
            Ok(StreamEvent::ContentDelta(text)),
            Ok(StreamEvent::MessageStop),
        ]))
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("echo", "echo-1")
    }
}

struct Clock;

#[async_trait]
impl Tool for Clock {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("clock", "The current time.", json!({"type": "object"}))
    }

    async fn call(&self, _input: Value, _context: &CallContext<'_>) -> ToolOutput {
        ToolOutput::ok("12:00")
    }
}

/// K-1: the caller's program compiles and runs; `Echo` answers the last
/// message's text and `Clock` answers `12:00`.
#[tokio::test]
async fn callers_provider_and_tool_run() {
    let request = CompletionRequest::new([Message::user("hi")])
        .max_tokens(256)
        .system("Be brief.");
    let _ = (request, Echo, Clock);

    let request = CompletionRequest::new([Message::user("hi")]);
    let events: Vec<_> = Echo.stream(request).collect().await;
    assert!(
        matches!(
            events.as_slice(),
            [Ok(StreamEvent::ContentDelta(text)), Ok(StreamEvent::MessageStop)] if text == "hi"
        ),
        "{events:?}"
    );
    assert_eq!(Echo.info(), ProviderInfo::new("echo", "echo-1"));
    assert_eq!(
        Clock.call(json!({}), &CallContext::default()).await,
        ToolOutput::ok("12:00")
    );
}

/// K-4: a request built from messages alone has the documented defaults.
#[test]
fn completion_request_defaults() {
    let request = CompletionRequest::new([Message::user("a")]);

    assert_eq!(request.model, ModelSelection::Default);
    assert_eq!(request.system, None);
    assert_eq!(request.max_tokens, 4096);
    assert!(request.tools.is_empty());
    assert_eq!(request.cache_breakpoints, []);
    assert_eq!(request.response_schema, None);
}

/// K-5: a message's text joins its text blocks in order and skips the rest.
#[test]
fn message_text_joins_text_blocks() {
    let message = Message::new(
        MessageRole::Assistant,
        vec![
            Content::Text { text: "a".into() },
            Content::ToolUse {
                id: "t".into(),
                name: "n".into(),
                input: json!({}),
            },
            Content::Text { text: "b".into() },
        ],
    );

    assert_eq!(message.text(), "ab");
}
