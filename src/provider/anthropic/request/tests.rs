//! Tests for the Messages API request body: rendering, cache breakpoints and
//! their validation, model selection.

use std::sync::Arc;

use super::MessagesBody;
use crate::provider::anthropic::test_support::{bp, text_request, user_text};
use crate::provider::error::RequestError;
use crate::provider::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, Message,
    MessageRole, ModelSelection, ResponseSchema, ToolDefinition,
};

/// The wire body of `request` as JSON, `default_model` for
/// `ModelSelection::Default`.
fn body(request: &CompletionRequest, default_model: &str) -> serde_json::Value {
    let body = MessagesBody::new(request, default_model).expect("valid body");
    serde_json::to_value(body).expect("body serializes")
}

/// The rejection `MessagesBody::new` gives `request`, if any.
fn rejection(request: &CompletionRequest) -> Option<RequestError> {
    MessagesBody::new(request, "m").err()
}

/// System prompt, model, limit and one text message reach the body; no
/// `stream`, no `tools`.
#[test]
fn body_basic() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: Some("You are helpful.".to_string()),
        messages: Arc::from(vec![user_text("Hello")]),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    };

    let body = body(&request, "claude-sonnet-4-6");

    assert_eq!(body["model"], "claude-sonnet-4-6");
    assert_eq!(body["max_tokens"], 1024);
    assert!(body.get("stream").is_none());
    assert_eq!(body["system"], "You are helpful.");
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    assert_eq!(body["messages"][0]["content"][0]["text"], "Hello");
    // No tools key when tools is empty
    assert!(body.get("tools").is_none());
}

/// Without breakpoints the body has no `cache_control` and a string
/// `system` (keys in sorted order).
#[test]
fn body_without_breakpoints_is_plain_json() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: Some("You are helpful.".to_string()),
        messages: Arc::from(vec![user_text("Hello")]),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: vec![],
        response_schema: None,
    };
    let expected = r#"{"max_tokens":1024,"messages":[{"content":[{"text":"Hello","type":"text"}],"role":"user"}],"model":"claude-sonnet-4-6","system":"You are helpful."}"#;
    let encoded =
        serde_json::to_string(&body(&request, "claude-sonnet-4-6")).expect("body serializes");

    assert_eq!(encoded, expected);
}

/// System/OneHour turns `system` into a cached text block;
/// Message(0)/FiveMinutes marks the last block of message 0.
#[test]
fn breakpoints_render_cache_control() {
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![user_text("Hi")]),
        tools: vec![],
        max_tokens: 16,
        response_schema: None,
        cache_breakpoints: vec![
            CacheBreakpoint {
                position: BreakpointPosition::System,
                ttl: CacheTtl::OneHour,
            },
            CacheBreakpoint {
                position: BreakpointPosition::Message(0),
                ttl: CacheTtl::FiveMinutes,
            },
        ],
    };
    let body = body(&request, "claude-sonnet-4-6");

    assert_eq!(
        body["system"],
        serde_json::json!([{
            "type": "text",
            "text": "S",
            "cache_control": {"type": "ephemeral", "ttl": "1h"}
        }])
    );
    assert_eq!(
        body["messages"][0]["content"][0]["cache_control"],
        serde_json::json!({"type": "ephemeral"})
    );
}

/// Each invalid layout and its rejection message.
#[test]
fn validate_breakpoints_messages() {
    let bp = |position| CacheBreakpoint {
        position,
        ttl: CacheTtl::FiveMinutes,
    };
    let req = |messages: Vec<Message>, breakpoints: Vec<CacheBreakpoint>| CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(messages),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: breakpoints,
        response_schema: None,
    };
    let user = |content: Vec<Content>| Message {
        role: MessageRole::User,
        content,
    };
    let hi = || {
        vec![Content::Text {
            text: "hi".to_string(),
        }]
    };
    let cases = vec![
        (
            req(vec![user(hi())], vec![bp(BreakpointPosition::Message(1))]),
            "cache breakpoint Message(1) out of range",
        ),
        (
            req(vec![user(vec![])], vec![bp(BreakpointPosition::Message(0))]),
            "cache breakpoint Message(0) points at an empty message",
        ),
        (
            req(vec![user(hi())], vec![bp(BreakpointPosition::System)]),
            "cache breakpoint System without a system prompt",
        ),
        (
            req(vec![user(hi())], vec![bp(BreakpointPosition::Tools)]),
            "cache breakpoint Tools without tools",
        ),
        (
            req(
                vec![user(hi())],
                vec![
                    bp(BreakpointPosition::Message(0)),
                    bp(BreakpointPosition::Message(0)),
                ],
            ),
            "duplicate cache breakpoint Message(0)",
        ),
    ];
    for (request, expected) in cases {
        match rejection(&request) {
            Some(error) => assert_eq!(error.to_string(), expected),
            None => panic!("expected InvalidRequest({expected:?}), got Ok"),
        }
    }

    assert!(rejection(&req(vec![user(hi())], vec![])).is_none());
}

/// A Tools breakpoint marks only the last tool definition.
#[test]
fn tools_breakpoint_marks_last_tool() {
    let tool = |name: &str| ToolDefinition {
        name: name.to_string(),
        description: "d".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    };
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![user_text("Hi")]),
        tools: vec![tool("a"), tool("b")],
        max_tokens: 16,
        response_schema: None,
        cache_breakpoints: vec![CacheBreakpoint {
            position: BreakpointPosition::Tools,
            ttl: CacheTtl::OneHour,
        }],
    };
    let body = body(&request, "claude-sonnet-4-6");

    assert!(body["tools"][0].get("cache_control").is_none());
    assert_eq!(
        body["tools"][1]["cache_control"],
        serde_json::json!({"type": "ephemeral", "ttl": "1h"})
    );
    assert_eq!(
        body["system"], "S",
        "no System breakpoint keeps system a string"
    );
}

/// Tools reach the body; no system prompt leaves `system` out.
#[test]
fn body_with_tools() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: None,
        messages: Arc::from(vec![user_text("List files")]),
        tools: vec![ToolDefinition {
            name: "bash".to_string(),
            description: "Run a command".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}}),
        }],
        max_tokens: 4096,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    };

    let body = body(&request, "claude-sonnet-4-6");

    assert!(body.get("system").is_none());
    let tools = body["tools"].as_array().expect("tools should be array");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "bash");
}

/// Tool use and tool result blocks keep their ids and flags.
#[test]
fn body_with_tool_result() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: None,
        messages: Arc::from(vec![
            user_text("List files"),
            Message {
                role: MessageRole::Assistant,
                content: vec![Content::ToolUse {
                    id: "toolu_1".to_string(),
                    name: "bash".to_string(),
                    input: serde_json::json!({"command": "ls"}),
                }],
            },
            Message {
                role: MessageRole::User,
                content: vec![Content::ToolResult {
                    tool_use_id: "toolu_1".to_string(),
                    content: "file.txt".to_string(),
                    is_error: false,
                }],
            },
        ]),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    };

    let body = body(&request, "claude-sonnet-4-6");

    assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(body["messages"][1]["content"][0]["id"], "toolu_1");
    assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(body["messages"][2]["content"][0]["is_error"], false);
}

/// `ModelSelection::Default` asks the provider to use its configured model:
/// a transport for `"opus"` sends `"opus"` in the wire payload.
#[test]
fn model_selection_default_uses_provider_configured_model() {
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![user_text("hi")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    };
    let body = body(&request, "opus");

    assert_eq!(
        body["model"], "opus",
        "Default must resolve to provider's configured model"
    );
}

/// `ModelSelection::Named(id)` overrides the provider's configured model —
/// the named id appears verbatim on the wire.
#[test]
fn model_selection_named_overrides_provider_configured_model() {
    let request = CompletionRequest {
        model: ModelSelection::Named("sonnet".to_string()),
        system: None,
        messages: Arc::from(vec![user_text("hi")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    };
    let body = body(&request, "opus");

    assert_eq!(
        body["model"], "sonnet",
        "Named override must replace the provider's configured model"
    );
}

/// 1h before 5m is fine.
#[test]
fn one_hour_before_five_minutes_is_ok() {
    let request = text_request(vec![
        bp(BreakpointPosition::Tools, CacheTtl::OneHour),
        bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
    ]);

    assert!(rejection(&request).is_none());
}

/// The check uses position rank, not vector order.
#[test]
fn rank_not_vector_order() {
    let request = text_request(vec![
        bp(BreakpointPosition::Message(0), CacheTtl::OneHour),
        bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
    ]);
    match rejection(&request) {
        Some(error) => assert_eq!(
            error.to_string(),
            "1h cache breakpoint after a 5m breakpoint"
        ),
        None => panic!("expected InvalidRequest, got Ok"),
    }
}

/// A typed answer is a structured-output format with the conformant
/// schema; no tool is added and none is forced.
#[test]
fn response_schema_becomes_output_config() {
    let mut request = text_request(Vec::new());
    request.tools.clear();
    request.response_schema = Some(ResponseSchema::new(
        "Summary",
        serde_json::json!({"type": "object", "properties": {
            "points": {"type": "array", "maxItems": 5, "items": {"type": "string"}}
        }}),
    ));

    let body = body(&request, "m");

    assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    let schema = &body["output_config"]["format"]["schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert!(schema["properties"]["points"].get("maxItems").is_none());
    assert!(body.get("tool_choice").is_none());
    assert!(body.get("tools").is_none());
    assert!(body.get("output_format").is_none());
}
