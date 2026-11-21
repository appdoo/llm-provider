//! Tests for one attempt against the Messages API: the streamed request,
//! its decoding, local rejection before any connection, and the provider
//! info.

use std::sync::Arc;

use tokio_stream::StreamExt;

use crate::provider::anthropic::test_support::{
    bp, sse_body, test_api, text_request, user_text, TEST_MODEL, TEXT_SSE,
};
use crate::provider::anthropic::Anthropic;
use crate::provider::error::Error;
use crate::provider::llm::Llm;
use crate::provider::test_stub::StubServer;
use crate::provider::StopReason;
use crate::provider::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, LlmProvider,
    Message, MessageRole, ModelSelection, Protocol, ProviderOptions,
};

/// The one-attempt API at `url` as a model, without retry.
fn one_attempt(url: &str) -> Llm {
    Llm::new(
        Arc::new(test_api(url, ProviderOptions::default())),
        TEST_MODEL,
    )
}

/// Five breakpoints fail locally: the stream's only item is
/// `InvalidRequest`, and the server never sees a connection.
#[tokio::test]
async fn too_many_breakpoints_fails_before_connecting() {
    let stub = StubServer::start(vec![]).await;
    let provider = test_api(&stub.url(), ProviderOptions::default());
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
        response_schema: None,
        sampling: crate::provider::Sampling::default(),
        cache_breakpoints: vec![
            bp(BreakpointPosition::System),
            bp(BreakpointPosition::Message(0)),
            bp(BreakpointPosition::Message(1)),
            bp(BreakpointPosition::Message(2)),
            bp(BreakpointPosition::Message(3)),
        ],
    };
    let items: Vec<_> = Protocol::stream(&provider, TEST_MODEL, request)
        .collect()
        .await;

    assert_eq!(items.len(), 1, "expected exactly one item, got {items:?}");
    match &items[0] {
        Err(Error::InvalidRequest(error)) => {
            assert_eq!(error.to_string(), "too many cache breakpoints: 5 (max 4)");
        }
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
    assert_eq!(stub.connection_count(), 0);
}

/// The info names the provider and its model and claims tools and streaming.
#[test]
fn provider_info() {
    let info = Anthropic::new("k").llm(TEST_MODEL).info();

    assert_eq!(info.name, "anthropic");
    assert_eq!(info.model, "claude-sonnet-5-5");
}

/// `complete()` sends `"stream": true` on the wire even though the request
/// carries no flag.
#[tokio::test]
async fn complete_sends_stream_true() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", TEXT_SSE.to_string()),
    )])
    .await;
    let provider = one_attempt(&stub.url());
    let request = CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![user_text("Hi")]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
        response_schema: None,
        sampling: crate::provider::Sampling::default(),
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
    let provider = one_attempt(&stub.url());
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
    let provider = test_api(&stub.url(), ProviderOptions::default());
    let request = text_request(vec![
        bp(BreakpointPosition::System, CacheTtl::FiveMinutes),
        bp(BreakpointPosition::Message(0), CacheTtl::OneHour),
    ]);
    let items: Vec<_> = Protocol::stream(&provider, TEST_MODEL, request)
        .collect()
        .await;

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
