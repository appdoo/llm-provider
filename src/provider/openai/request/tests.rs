//! Tests for the `/chat/completions` body: messages, tool calls and
//! results, tools, dropped cache breakpoints.

use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::provider::request::{BreakpointPosition, CacheBreakpoint, CacheTtl};

/// A request with `messages`, the default model and 256 output tokens.
fn request(system: Option<&str>, messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: system.map(str::to_string),
        messages: Arc::from(messages),
        tools: vec![],
        max_tokens: 256,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    }
}

/// The body for `request` with provider model `model`, as JSON.
fn body(request: &CompletionRequest, model: &str) -> Value {
    serde_json::to_value(ChatBody::new(request, model).expect("body")).expect("json")
}

/// The `messages` of the body for `request`, as JSON.
fn messages(request: &CompletionRequest) -> Value {
    let body = ChatBody::new(request, "m").expect("body");
    serde_json::to_value(&body.messages).expect("json")
}

/// A user message of `blocks`.
fn user(blocks: Vec<Content>) -> Message {
    Message {
        role: MessageRole::User,
        content: blocks,
    }
}

/// A tool result for `call_1`.
fn result(content: &str, is_error: bool) -> Content {
    Content::ToolResult {
        tool_use_id: "call_1".to_string(),
        content: content.to_string(),
        is_error,
    }
}

/// The system prompt comes first, a `Named` model wins over the provider
/// default, streaming asks for usage; A-7: no tools means no `tools` key.
#[test]
fn system_user_named_model_stream_options() {
    let mut req = request(Some("Be brief"), vec![Message::user("Hi")]);
    req.model = ModelSelection::Named("gpt-4o-mini".to_string());

    let expected = json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "system", "content": "Be brief"},
            {"role": "user", "content": "Hi"}
        ],
        "max_tokens": 256,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    assert_eq!(body(&req, "provider-default"), expected);
}

/// An assistant tool call becomes `tool_calls` with JSON-string arguments
/// and `content: null`; its result becomes its own `tool` message.
#[test]
fn tool_use_and_tool_result_projection() {
    let call = Message {
        role: MessageRole::Assistant,
        content: vec![Content::ToolUse {
            id: "call_1".to_string(),
            name: "read".to_string(),
            input: json!({"path": "a.rs"}),
        }],
    };
    let req = request(None, vec![call, user(vec![result("fn main(){}", false)])]);

    let expected = json!([
        {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "type": "function",
                "id": "call_1",
                "function": {"name": "read", "arguments": "{\"path\":\"a.rs\"}"}
            }]
        },
        {"role": "tool", "tool_call_id": "call_1", "content": "fn main(){}"}
    ]);
    assert_eq!(messages(&req), expected);
}

/// A failed result is the exact JSON string
/// `{"error":true,"content":"<text>"}`.
#[test]
fn error_tool_result_content_is_exact_json_string() {
    let req = request(None, vec![user(vec![result("no such file", true)])]);

    let expected = json!([{
        "role": "tool",
        "tool_call_id": "call_1",
        "content": "{\"error\":true,\"content\":\"no such file\"}"
    }]);
    assert_eq!(messages(&req), expected);
}

/// Tool results precede the remaining text, joined by a blank line; A-7: a
/// tool becomes a `function` definition named after it; `Default` uses the
/// provider model.
#[test]
fn user_text_after_tool_results_and_tools_projection() {
    let text = |t: &str| Content::Text {
        text: t.to_string(),
    };
    let mut req = request(
        None,
        vec![user(vec![text("a"), result("r1", false), text("b")])],
    );
    req.tools = vec![ToolDefinition {
        name: "read".to_string(),
        description: "Read a file".to_string(),
        input_schema: json!({"type": "object"}),
    }];

    let expected = json!({
        "model": "llama3.2",
        "messages": [
            {"role": "tool", "tool_call_id": "call_1", "content": "r1"},
            {"role": "user", "content": "a\n\nb"}
        ],
        "max_tokens": 256,
        "stream": true,
        "stream_options": {"include_usage": true},
        "tools": [{
            "type": "function",
            "function": {
                "name": "read",
                "description": "Read a file",
                "parameters": {"type": "object"}
            }
        }]
    });
    assert_eq!(body(&req, "llama3.2"), expected);
}

/// Cache breakpoints are dropped: no `cache_control` anywhere.
#[test]
fn cache_breakpoints_are_dropped() {
    let mut req = request(Some("S"), vec![Message::user("Hi")]);
    req.cache_breakpoints = vec![CacheBreakpoint {
        position: BreakpointPosition::System,
        ttl: CacheTtl::OneHour,
    }];

    let serialized = body(&req, "m").to_string();

    assert!(!serialized.contains("cache_control"), "got {serialized}");
    assert!(serialized.contains("\"content\":\"S\""));
}
