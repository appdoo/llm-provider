//! Tests for the Anthropic client: builder, headers, wire body and breakpoints, stream decoding, retry through the front type and batches.

use std::sync::Arc;
use std::time::Duration;

use tokio_stream::StreamExt;

use super::api::AnthropicApi;
use super::request::MessagesBody;
use super::Anthropic;
use crate::provider::auth::AuthMethod;
use crate::provider::error::{Error, RequestError};
use crate::provider::retry::RetryPolicy;
use crate::provider::test_stub::{StubBody, StubReply, StubServer};
use crate::provider::traits::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, LlmProvider,
    Message, MessageRole, ModelSelection, ProviderOptions, ToolDefinition,
};
use crate::provider::{StopReason, StreamEvent};
use crate::transport::Http;

const FRONT_SSE: &str = include_str!("../../../tests/fixtures/anthropic_text.sse");

/// A transport for `model` at `base_url` with the given options.
fn api_with(model: &str, key: &str, base_url: &str, options: ProviderOptions) -> AnthropicApi {
    AnthropicApi::new(
        Http::new(options),
        AuthMethod::from(key),
        model.to_string(),
        base_url,
    )
}

/// A transport for `model` at `base_url` with the default options.
fn api_at(model: &str, key: &str, base_url: &str) -> AnthropicApi {
    api_with(model, key, base_url, ProviderOptions::default())
}

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

fn user_text(text: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: vec![Content::Text {
            text: text.to_string(),
        }],
    }
}

// --- wire body ---

#[test]
fn build_request_body_basic() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: Some("You are helpful.".to_string()),
        messages: Arc::from(vec![user_text("Hello")]),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: Vec::new(),
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

/// Without breakpoints the wire body is the plain encoding (golden recorded
/// from the unbroken-up request; keys compared in sorted order).
#[test]
fn no_breakpoints_body_matches_pre_change_golden() {
    let request = CompletionRequest {
        model: ModelSelection::Named("claude-sonnet-4-6".to_string()),
        system: Some("You are helpful.".to_string()),
        messages: Arc::from(vec![user_text("Hello")]),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: vec![],
    };
    let golden = r#"{"max_tokens":1024,"messages":[{"content":[{"text":"Hello","type":"text"}],"role":"user"}],"model":"claude-sonnet-4-6","system":"You are helpful."}"#;
    let encoded =
        serde_json::to_string(&body(&request, "claude-sonnet-4-6")).expect("body serializes");
    assert_eq!(encoded, golden);
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

/// Five breakpoints fail locally: the stream's only item is
/// `InvalidRequest`, and the server never sees a connection.
#[tokio::test]
async fn too_many_breakpoints_fails_before_connecting() {
    let stub = StubServer::start(vec![]).await;
    let provider = api_at("claude-sonnet-4-6", "k", &stub.url());
    let bp = |position| CacheBreakpoint {
        position,
        ttl: CacheTtl::FiveMinutes,
    };
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![
            user_text("a"),
            Message {
                role: MessageRole::Assistant,
                content: vec![Content::Text {
                    text: "b".to_string(),
                }],
            },
            user_text("c"),
        ]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: vec![
            bp(BreakpointPosition::System),
            bp(BreakpointPosition::Message(0)),
            bp(BreakpointPosition::Message(1)),
            bp(BreakpointPosition::Message(2)),
            bp(BreakpointPosition::Message(3)),
        ],
    };
    let items: Vec<_> = provider.stream(request).collect().await;
    assert_eq!(items.len(), 1, "expected exactly one item, got {items:?}");
    match &items[0] {
        Err(Error::InvalidRequest(error)) => {
            assert_eq!(error.to_string(), "too many cache breakpoints: 5 (max 4)");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert_eq!(stub.connection_count(), 0);
}

/// Validation table — every remaining rejection message.
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

#[test]
fn build_request_body_with_tools() {
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
    };

    let body = body(&request, "claude-sonnet-4-6");

    assert!(body.get("system").is_none());
    let tools = body["tools"].as_array().expect("tools should be array");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "bash");
}

#[test]
fn build_request_body_with_tool_result() {
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
    };

    let body = body(&request, "claude-sonnet-4-6");

    assert_eq!(body["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(body["messages"][1]["content"][0]["id"], "toolu_1");
    assert_eq!(body["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(body["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(body["messages"][2]["content"][0]["is_error"], false);
}

#[test]
fn provider_info() {
    let provider = api_at("claude-sonnet-4-6", "k", super::client::DEFAULT_BASE_URL);
    let info = provider.info();
    assert_eq!(info.name, "anthropic");
    assert_eq!(info.model, "claude-sonnet-4-6");
    assert!(info.supports_tools);
    assert!(info.supports_streaming);
}

// --- ModelSelection resolution at request-build time ---

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
    };
    let body = body(&request, "opus");
    assert_eq!(
        body["model"], "sonnet",
        "Named override must replace the provider's configured model"
    );
}

/// No model sentinel built from `String::new()` remains in this crate's
/// source. The scan excludes this very test file (which references the
/// pattern in literal strings).
///
/// `#[cfg(unix)]` because the test shells out to `grep`.
#[cfg(unix)]
#[test]
fn no_string_new_model_sentinel_in_src() {
    use std::process::Command;
    let src_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    // The needle is split across two substrings so this source line itself
    // does not form the forbidden pattern.
    let needle = format!("model: {}::new()", "String");
    let out = Command::new("grep")
        .arg("-rn")
        .arg(&needle)
        .arg(src_dir)
        .output()
        .expect("grep must be available on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let this_file = file!();
    let real_hits: Vec<&str> = stdout.lines().filter(|l| !l.contains(this_file)).collect();
    assert!(
        real_hits.is_empty(),
        "no `model: String::new()` sentinels must remain; found:\n{}",
        real_hits.join("\n")
    );
}

// --- stop reason, TTL order ---

fn sse_body(stop_reason: &str) -> String {
    format!(
        "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":3}}}}}}\n\n\
         event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"1, 2\"}}}}\n\n\
         event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

fn text_request(breakpoints: Vec<CacheBreakpoint>) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![user_text("Count from 1 to 50")]),
        tools: vec![ToolDefinition {
            name: "t".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }],
        max_tokens: 5,
        cache_breakpoints: breakpoints,
    }
}

fn bp(position: BreakpointPosition, ttl: CacheTtl) -> CacheBreakpoint {
    CacheBreakpoint { position, ttl }
}

/// `complete()` sends `"stream": true` on the wire even though the request
/// carries no flag.
#[tokio::test]
async fn complete_sends_stream_true() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", FRONT_SSE.to_string()),
    )])
    .await;
    let provider = api_at("claude-sonnet-4-6", "sk-test", &stub.url());
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![user_text("Hi")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
    };
    provider.complete(request).await.expect("complete");
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).expect("json body");
    assert_eq!(body["stream"], true);
}

/// `message_delta` with `max_tokens` makes `complete().stop` `MaxTokens`.
#[tokio::test]
async fn complete_reports_max_tokens() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", sse_body("max_tokens")),
    )])
    .await;
    let provider = api_at("claude-sonnet-4-6", "k", &stub.url());
    let completion = provider
        .complete(text_request(vec![]))
        .await
        .expect("complete");
    assert_eq!(completion.stop, StopReason::MaxTokens);
    assert_eq!(completion.text, "1, 2");
}

/// A 1h breakpoint after a 5m one fails before any connection.
#[tokio::test]
async fn one_hour_after_five_minutes_fails_before_connecting() {
    let stub = StubServer::start(vec![]).await;
    let provider = api_at("claude-sonnet-4-6", "k", &stub.url());
    let request = text_request(vec![
        bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
        bp(BreakpointPosition::Message(0), CacheTtl::OneHour),
    ]);
    let items: Vec<_> = provider.stream(request).collect().await;
    assert_eq!(items.len(), 1, "got {items:?}");
    match &items[0] {
        Err(Error::InvalidRequest(error)) => {
            assert_eq!(
                error.to_string(),
                "1h cache breakpoint after a 5m breakpoint"
            );
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert_eq!(stub.connection_count(), 0);
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

// --- idle gap (real time, 1 s; paused time raced the stub's socket) ---

fn one_second_idle() -> ProviderOptions {
    ProviderOptions::default()
        .connect_timeout(Duration::from_secs(30))
        .idle_gap(Duration::from_secs(1))
}

async fn collect_idle(body: StubBody) -> Vec<Result<StreamEvent, Error>> {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![StubReply {
            status: 200,
            content_type: "text/event-stream",
            headers: Vec::new(),
            body,
        }],
    )])
    .await;
    let provider = api_with("m", "test-key", &stub.url(), one_second_idle());
    let started = std::time::Instant::now();
    let items: Vec<_> = provider.stream(text_request(vec![])).collect().await;
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "took {:?}",
        started.elapsed()
    );
    items
}

fn assert_idle_after_1s(items: &[Result<StreamEvent, Error>]) {
    assert_eq!(items.len(), 1, "items: {items:?}");
    assert!(
        matches!(&items[0], Err(Error::IdleTimeout { after }) if *after == Duration::from_secs(1)),
        "got {:?}",
        items[0]
    );
}

/// Headers arrive, then silence: `IdleTimeout` after 1 s.
#[tokio::test]
async fn idle_gap_between_chunks() {
    let items = collect_idle(StubBody::HeadersThenHang).await;
    assert_idle_after_1s(&items);
}

/// No headers at all: `IdleTimeout` after 1 s.
#[tokio::test]
async fn idle_gap_before_headers() {
    let items = collect_idle(StubBody::Hang).await;
    assert_idle_after_1s(&items);
}

fn hi_request() -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![user_text("Hi")]),
        tools: vec![],
        max_tokens: 64,
        cache_breakpoints: Vec::new(),
    }
}

/// A 302 is not followed: `Api` with status 302, the target is never hit.
#[tokio::test]
async fn redirect_is_not_followed() {
    let b = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", FRONT_SSE.to_string()),
    )])
    .await;
    let a = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![StubReply {
            status: 302,
            content_type: "text/plain",
            headers: vec![("location", format!("{}/v1/messages", b.url()))],
            body: StubBody::Full(String::new()),
        }],
    )])
    .await;
    let provider = api_at("claude-sonnet-4-6", "sk-test", &a.url());
    let err = provider.complete(hi_request()).await.expect_err("302");
    assert_eq!(err.kind(), crate::provider::error::ErrorKind::Other);
    match err {
        Error::Api(e) => assert_eq!(e.status, Some(302)),
        other => panic!("expected Api, got {other:?}"),
    }
    assert_eq!(a.requests().len(), 1);
    assert_eq!(b.requests().len(), 0);
}

/// A 200 KiB error body is cut to the 64 KiB cap.
#[tokio::test]
async fn error_body_is_capped() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (500, "text/plain", "a".repeat(200 * 1024)),
    )])
    .await;
    let provider = api_at("claude-sonnet-4-6", "sk-test", &stub.url());
    match provider.complete(hi_request()).await.expect_err("500") {
        Error::Api(e) => {
            assert_eq!(e.status, Some(500));
            assert_eq!(e.message, "a".repeat(65_536));
        }
        other => panic!("expected Api, got {other:?}"),
    }
}

/// An error body that never arrives ends in `IdleTimeout`.
#[tokio::test]
async fn error_body_idle_timeout() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![StubReply {
            status: 500,
            content_type: "application/json",
            headers: vec![],
            body: StubBody::HeadersThenHang,
        }],
    )])
    .await;
    let provider = api_with("m", "sk-test", &stub.url(), one_second_idle());
    let started = std::time::Instant::now();
    let err = provider.complete(hi_request()).await.expect_err("hang");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(
        matches!(err, Error::IdleTimeout { after } if after == Duration::from_secs(1)),
        "got {err:?}"
    );
}

// --- front type `Anthropic` ---

async fn front_stub() -> StubServer {
    StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", FRONT_SSE.to_string()),
    )])
    .await
}

fn body_of(stub: &StubServer, i: usize) -> serde_json::Value {
    serde_json::from_str(&stub.requests()[i].body).expect("json body")
}

/// A1: `ask` returns the text and sends key, model, default limit, user message.
#[tokio::test]
async fn front_ask_sends_key_and_defaults() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .build();
    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("sk-test")
    );
    let body = body_of(&stub, 0);
    assert_eq!(body["model"], "claude-x");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(
        body["messages"],
        serde_json::json!([{"role":"user","content":[{"type":"text","text":"Hello"}]}])
    );
}

/// A2: the builder's system, model and limit reach the wire.
#[tokio::test]
async fn front_request_builder_overrides() {
    let stub = front_stub().await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .build();
    let done = claude
        .request("Hello")
        .system("Be brief.")
        .model("claude-y")
        .max_tokens(256)
        .send()
        .await
        .expect("send");
    assert_eq!(done.text, "Hi");
    let body = body_of(&stub, 0);
    assert_eq!(body["system"], "Be brief.");
    assert_eq!(body["model"], "claude-y");
    assert_eq!(body["max_tokens"], 256);
}

fn overloaded_then_ok() -> Vec<((&'static str, &'static str), Vec<StubReply>)> {
    vec![(
        ("POST", "/v1/messages"),
        vec![
            StubReply::full(
                529,
                "application/json",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
            StubReply::full(200, "text/event-stream", FRONT_SSE),
        ],
    )]
}

/// A5: retry is on by default-shaped policy; a 529 is retried.
#[tokio::test]
async fn front_retries_overloaded() {
    let stub = StubServer::start_replies(overloaded_then_ok()).await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .retry(
            RetryPolicy::default()
                .max_attempts(4)
                .base(Duration::from_millis(1))
                .cap(Duration::from_millis(10)),
        )
        .build();
    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    assert_eq!(stub.requests().len(), 2);
}

/// A5b: `RetryPolicy::none()` gives up after the first attempt.
#[tokio::test]
async fn front_retry_none_gives_up() {
    let stub = StubServer::start_replies(overloaded_then_ok()).await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .retry(RetryPolicy::none())
        .build();
    let err = claude.ask("Hello").await.expect_err("exhausted");
    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
        "{err:?}"
    );
    assert_eq!(err.kind(), crate::provider::ErrorKind::Overloaded);
    assert_eq!(stub.requests().len(), 1);
}

/// A6: a Bearer credential sends OAuth headers and no `x-api-key`.
#[tokio::test]
async fn front_bearer_sends_oauth_headers() {
    let stub = front_stub().await;
    let claude = Anthropic::builder(AuthMethod::Bearer("oat-test".into()), "claude-x")
        .base_url(stub.url())
        .build();
    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");
    let h = &stub.requests()[0].headers;
    assert_eq!(
        h.get("authorization").map(String::as_str),
        Some("Bearer oat-test")
    );
    assert_eq!(
        h.get("anthropic-beta").map(String::as_str),
        Some("oauth-2025-04-20")
    );
    assert!(!h.contains_key("x-api-key"));
}

/// A9: policy defaults and `none()`. The attempt count is private now, so
/// the policies are compared whole.
#[test]
fn front_policy_defaults() {
    assert_eq!(RetryPolicy::default(), RetryPolicy::none().max_attempts(4));
    assert_eq!(RetryPolicy::none(), RetryPolicy::default().max_attempts(1));
    assert_eq!(Anthropic::new("k", "m").policy(), RetryPolicy::default());
}

/// A10: Debug shows the model, never the credential.
#[test]
fn front_debug_hides_credential() {
    let shown = format!("{:?}", Anthropic::new("sk-secret", "m"));
    assert!(!shown.contains("sk-secret"), "{shown}");
    assert!(shown.contains("\"m\""), "{shown}");
}
