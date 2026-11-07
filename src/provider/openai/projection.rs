//! Projection of a `CompletionRequest` (Anthropic shape) onto the
//! OpenAI `/chat/completions` request body. Pure functions, no I/O.

use serde::Serialize;
use serde_json::{json, Value};

use crate::provider::traits::*;

/// The `content` of a tool message for an `is_error: true` result. A struct
/// (not `json!`) keeps the field order `error`, `content` on the wire.
#[derive(Serialize)]
struct ToolErrorContent<'a> {
    error: bool,
    content: &'a str,
}

/// Build the `/chat/completions` body for `req`. `model` is the provider's
/// own model, used when `req.model` is `ModelSelection::Default`.
/// Cache breakpoints are dropped.
pub(crate) fn project_request(req: &CompletionRequest, model: &str) -> Value {
    let effective_model = match &req.model {
        ModelSelection::Default => model.to_string(),
        ModelSelection::Named(id) => id.clone(),
    };

    let mut messages: Vec<Value> = Vec::new();
    if let Some(system) = &req.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for message in req.messages.iter() {
        match message.role {
            MessageRole::User => project_user(message, &mut messages),
            MessageRole::Assistant => messages.push(project_assistant(message)),
        }
    }

    let mut body = json!({
        "model": effective_model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    body
}

/// Each `ToolResult` becomes its own `tool` message, in block order; the
/// remaining text blocks follow as one `user` message.
fn project_user(message: &Message, out: &mut Vec<Value>) {
    let mut texts: Vec<&str> = Vec::new();
    for block in &message.content {
        match block {
            Content::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let content = if *is_error {
                    serde_json::to_string(&ToolErrorContent {
                        error: true,
                        content,
                    })
                    .unwrap_or_default()
                } else {
                    content.clone()
                };
                out.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": content,
                }));
            }
            Content::Text { text } => texts.push(text),
            // A user message carries no tool calls on the chat/completions wire.
            Content::ToolUse { .. } => {}
        }
    }
    if !texts.is_empty() {
        out.push(json!({ "role": "user", "content": texts.join("\n\n") }));
    }
}

fn project_assistant(message: &Message) -> Value {
    let mut texts: Vec<&str> = Vec::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    for block in &message.content {
        match block {
            Content::Text { text } => texts.push(text),
            Content::ToolUse { id, name, input } => tool_calls.push(json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(input).unwrap_or_default(),
                }
            })),
            // An assistant message carries no tool results.
            Content::ToolResult { .. } => {}
        }
    }
    let content = if texts.is_empty() {
        Value::Null
    } else {
        Value::String(texts.join("\n\n"))
    };
    let mut out = json!({ "role": "assistant", "content": content });
    if !tool_calls.is_empty() {
        out["tool_calls"] = Value::Array(tool_calls);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn request(system: Option<&str>, messages: Vec<Message>) -> CompletionRequest {
        CompletionRequest {
            model: ModelSelection::Default,
            system: system.map(str::to_string),
            messages: Arc::from(messages),
            tools: vec![],
            max_tokens: 256,
            cache_breakpoints: Vec::new(),
        }
    }

    /// System prompt becomes the first message; `Named` model wins
    /// over the provider default; streaming asks for usage.
    #[test]
    fn system_user_named_model_stream_options() {
        let mut req = request(
            Some("Be brief"),
            vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }],
        );
        req.model = ModelSelection::Named("gpt-4o-mini".to_string());

        let body = project_request(&req, "provider-default");

        assert_eq!(
            body["messages"],
            json!([
                {"role": "system", "content": "Be brief"},
                {"role": "user", "content": "Hi"}
            ])
        );
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 256);
        assert!(body.get("tools").is_none(), "empty tools are omitted");
    }

    /// `complete()` always sends the stream flag and the usage option.
    #[tokio::test]
    async fn complete_sends_stream_and_usage_option() {
        use crate::provider::{test_stub::StubServer, LlmProvider, OpenAiChatApi};
        let stub = StubServer::start(vec![(
            ("POST", "/chat/completions"),
            (
                200,
                "text/event-stream",
                include_str!("../../../tests/fixtures/openai_text.sse").to_string(),
            ),
        )])
        .await;
        let provider = OpenAiChatApi::new(&stub.url(), "m", None);
        let req = request(
            None,
            vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }],
        );
        provider.complete(req).await.expect("complete");
        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        let body: Value = serde_json::from_str(&requests[0].body).expect("json body");
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"], json!({"include_usage": true}));
    }

    /// Assistant tool call → `tool_calls` with JSON-string
    /// arguments and `content: null`; the result → its own `tool` message.
    #[test]
    fn tool_use_and_tool_result_projection() {
        let req = request(
            None,
            vec![
                Message {
                    role: MessageRole::Assistant,
                    content: vec![Content::ToolUse {
                        id: "call_1".to_string(),
                        name: "read".to_string(),
                        input: json!({"path": "a.rs"}),
                    }],
                },
                Message {
                    role: MessageRole::User,
                    content: vec![Content::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: "fn main(){}".to_string(),
                        is_error: false,
                    }],
                },
            ],
        );

        let body = project_request(&req, "m");
        let messages = body["messages"].as_array().expect("messages array");
        assert_eq!(messages.len(), 2, "no extra user message: {messages:?}");

        let assistant = &messages[0];
        assert_eq!(assistant["role"], "assistant");
        assert_eq!(assistant["content"], Value::Null);
        assert_eq!(assistant["tool_calls"][0]["id"], "call_1");
        assert_eq!(assistant["tool_calls"][0]["type"], "function");
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "read");
        assert_eq!(
            assistant["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"a.rs\"}"
        );

        assert_eq!(
            messages[1],
            json!({"role": "tool", "tool_call_id": "call_1", "content": "fn main(){}"})
        );
    }

    /// An error result is wrapped as the exact JSON string
    /// `{"error":true,"content":"<text>"}`.
    #[test]
    fn error_tool_result_content_is_exact_json_string() {
        let req = request(
            None,
            vec![Message {
                role: MessageRole::User,
                content: vec![Content::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "no such file".to_string(),
                    is_error: true,
                }],
            }],
        );

        let body = project_request(&req, "m");
        assert_eq!(
            body["messages"],
            json!([{
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "{\"error\":true,\"content\":\"no such file\"}"
            }])
        );
    }

    /// Tool results precede the remaining text, which is joined by `\n\n`;
    /// tools project to `function` definitions; `Default` uses the
    /// provider model.
    #[test]
    fn user_text_after_tool_results_and_tools_projection() {
        let mut req = request(
            None,
            vec![Message {
                role: MessageRole::User,
                content: vec![
                    Content::Text {
                        text: "a".to_string(),
                    },
                    Content::ToolResult {
                        tool_use_id: "call_1".to_string(),
                        content: "r1".to_string(),
                        is_error: false,
                    },
                    Content::Text {
                        text: "b".to_string(),
                    },
                ],
            }],
        );
        req.tools = vec![ToolDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            input_schema: json!({"type": "object"}),
        }];

        let body = project_request(&req, "llama3.2");
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(
            body["messages"],
            json!([
                {"role": "tool", "tool_call_id": "call_1", "content": "r1"},
                {"role": "user", "content": "a\n\nb"}
            ])
        );
        assert_eq!(
            body["tools"],
            json!([{
                "type": "function",
                "function": {
                    "name": "read",
                    "description": "Read a file",
                    "parameters": {"type": "object"}
                }
            }])
        );
    }

    /// Cache breakpoints are dropped: no `cache_control` anywhere.
    #[test]
    fn cache_breakpoints_are_dropped() {
        let mut req = request(
            Some("S"),
            vec![Message {
                role: MessageRole::User,
                content: vec![Content::Text {
                    text: "Hi".to_string(),
                }],
            }],
        );
        req.cache_breakpoints = vec![CacheBreakpoint {
            position: BreakpointPosition::System,
            ttl: CacheTtl::OneHour,
        }];

        let serialized = project_request(&req, "m").to_string();
        assert!(
            !serialized.contains("cache_control"),
            "unexpected cache_control in {serialized}"
        );
        assert!(serialized.contains("\"content\":\"S\""));
    }
}
