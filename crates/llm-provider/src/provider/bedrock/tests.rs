//! Tests for Bedrock as one channel with two protocols: routes, credential
//! headers, the region check, the shared pool, prompt caching, retry,
//! timeouts, key isolation and the error envelope.

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::Bedrock;
use crate::config::ConfigError;
use crate::provider::anthropic::Anthropic;
use crate::provider::auth::AuthMethod;
use crate::provider::error::{Error, ErrorKind};
use crate::provider::retry::RetryPolicy;
use crate::provider::test_stub::{
    RecordedRequest, RecordingSleeper, StubBody, StubReply, StubServer,
};
use crate::provider::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, LlmProvider,
    Message, MessageRole, ModelSelection, ProviderOptions,
};

const ANTHROPIC_SSE: &str = include_str!("../../../tests/fixtures/anthropic_text.sse");
const OPENAI_SSE: &str = include_str!("../../../tests/fixtures/openai_text.sse");
const MESSAGES: (&str, &str) = ("POST", "/anthropic/v1/messages");
const CHAT: (&str, &str) = ("POST", "/openai/v1/chat/completions");
const REGION: &str = "eu-central-1";
const M: &str = Bedrock::EU_CLAUDE_HAIKU_5_5;
const N: &str = Bedrock::KIMI_K3;

/// A stub serving both Bedrock routes with a text answer.
async fn both_routes() -> StubServer {
    StubServer::start(vec![
        (
            MESSAGES,
            (200, "text/event-stream", ANTHROPIC_SSE.to_string()),
        ),
        (CHAT, (200, "text/event-stream", OPENAI_SSE.to_string())),
    ])
    .await
}

/// Bedrock with `key` in [`REGION`] at the stub, without retry.
fn bedrock_at(key: impl Into<AuthMethod>, stub: &StubServer) -> Bedrock {
    Bedrock::builder(key, REGION)
        .expect("a valid region")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
}

/// The value of header `name` on `request`.
fn header<'a>(request: &'a RecordedRequest, name: &str) -> Option<&'a str> {
    request.headers.get(name).map(String::as_str)
}

/// P-2: Claude on Bedrock posts under `/anthropic` with the key as
/// `x-api-key`, and sends neither `authorization` nor `anthropic-beta`.
#[tokio::test]
async fn anthropic_route_sends_the_key_as_x_api_key() {
    let stub = both_routes().await;

    let llm = bedrock_at("ABSK-x", &stub).anthropic().llm(M);
    llm.ask("x").await.expect("ask");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/anthropic/v1/messages");
    assert_eq!(header(&requests[0], "x-api-key"), Some("ABSK-x"));
    assert_eq!(header(&requests[0], "authorization"), None);
    assert_eq!(header(&requests[0], "anthropic-beta"), None);
}

/// P-3: the OpenAI route posts under `/openai/v1` with the key as a bearer
/// token and no `x-api-key`.
#[tokio::test]
async fn openai_route_sends_the_key_as_bearer() {
    let stub = both_routes().await;

    let llm = bedrock_at("ABSK-x", &stub).openai().llm(N);
    llm.ask("x").await.expect("ask");

    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/openai/v1/chat/completions");
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer ABSK-x"));
    assert_eq!(header(&requests[0], "x-api-key"), None);
}

/// P-4: an invalid region is refused when built, before any I/O, so
/// neither protocol can send.
#[test]
fn invalid_region_fails_both_protocols_before_io() {
    let error = Bedrock::builder("ABSK-x", "Frankfurt").expect_err("invalid region");

    assert_eq!(error, ConfigError::InvalidRegion("Frankfurt".to_string()));
}

/// E5: `Bedrock::new` and `Bedrock::builder` check the region where it is
/// named; a valid one builds.
#[test]
fn region_is_checked_when_built() {
    let upper = Bedrock::new("k", "EU").expect_err("upper case");
    let underscores = Bedrock::builder("k", "eu_central_1").expect_err("underscores");
    let valid = Bedrock::new("k", "eu-central-1");

    assert_eq!(upper, ConfigError::InvalidRegion("EU".to_string()));
    assert_eq!(
        underscores,
        ConfigError::InvalidRegion("eu_central_1".to_string())
    );
    assert!(valid.is_ok(), "{valid:?}");
}

/// P-5b: both protocols send through the caller's one HTTP client.
#[tokio::test]
async fn both_protocols_share_the_http_client() {
    let stub = both_routes().await;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-pool", reqwest::header::HeaderValue::from_static("1"));
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("client");
    let bedrock = Bedrock::builder("ABSK-x", REGION)
        .expect("a valid region")
        .http_client(client)
        .base_url(stub.url())
        .expect("a valid URL")
        .build();

    bedrock
        .anthropic()
        .llm(M)
        .ask("a")
        .await
        .expect("anthropic");
    bedrock.openai().llm(N).ask("b").await.expect("openai");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|r| header(r, "x-pool") == Some("1")));
}

/// A request whose system prompt carries a one-hour cache breakpoint.
fn cached_request() -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: "x".to_string(),
            }],
        }]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: vec![CacheBreakpoint {
            position: BreakpointPosition::System,
            ttl: CacheTtl::OneHour,
        }],
        response_schema: None,
        sampling: crate::provider::Sampling::default(),
    }
}

/// An answer "Hi" whose usage read 45859 tokens from the cache.
fn cache_read_sse() -> String {
    let start = serde_json::json!({"type":"message_start","message":{"usage":{
        "input_tokens":3,"cache_creation_input_tokens":0,"cache_read_input_tokens":45859}}});
    let delta = serde_json::json!({"type":"content_block_delta","index":0,
        "delta":{"type":"text_delta","text":"Hi"}});
    let end = serde_json::json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
        "usage":{"output_tokens":1}});
    format!(
        "event: message_start\ndata: {start}\n\n\
         event: content_block_delta\ndata: {delta}\n\n\
         event: message_delta\ndata: {end}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

/// P-9: a one-hour breakpoint reaches Bedrock as `cache_control`, and the
/// cache-read count comes back in the usage.
#[tokio::test]
async fn prompt_cache_works_on_bedrock() {
    let reply = (200, "text/event-stream", cache_read_sse());
    let stub = StubServer::start(vec![(MESSAGES, reply)]).await;

    let llm = bedrock_at("ABSK-x", &stub).anthropic().llm(M);
    let completion = llm.complete(cached_request()).await.expect("complete");

    let body: serde_json::Value =
        serde_json::from_str(&stub.requests()[0].body).expect("json body");
    assert_eq!(
        body["system"][0]["cache_control"],
        serde_json::json!({"type":"ephemeral","ttl":"1h"})
    );
    assert_eq!(completion.usage.cache_read_input_tokens, 45859);
}

/// K2: `cache(FiveMinutes)` on the Anthropic route marks the system prompt
/// and the last message block, and nothing else.
#[tokio::test]
async fn request_cache_marks_two_blocks_on_bedrock() {
    let stub = both_routes().await;
    let llm = bedrock_at("ABSK-x", &stub).anthropic().llm(M);

    llm.request("hi")
        .system("S")
        .cache(CacheTtl::FiveMinutes)
        .send()
        .await
        .expect("send");

    let request = &stub.requests()[0];
    assert_eq!(request.path, "/anthropic/v1/messages");
    let body: serde_json::Value = serde_json::from_str(&request.body).expect("json body");
    let marker = serde_json::json!({"type":"ephemeral"});
    assert_eq!(body["system"][0]["cache_control"], marker);
    assert_eq!(body["messages"][0]["content"][0]["cache_control"], marker);
    assert_eq!(request.body.matches("cache_control").count(), 2);
}

/// A stub answering the Messages route with 529, then a text answer.
async fn overloaded_then_ok() -> StubServer {
    let overloaded =
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
    StubServer::start_replies(vec![(
        MESSAGES,
        vec![
            StubReply::full(529, "application/json", overloaded),
            StubReply::full(200, "text/event-stream", ANTHROPIC_SSE),
        ],
    )])
    .await
}

/// P-14: the endpoint's retry policy rules: `none()` gives up after one
/// 529, the default retries it.
#[tokio::test]
async fn retry_policy_belongs_to_the_endpoint() {
    let once = overloaded_then_ok().await;
    let llm = bedrock_at("ABSK-x", &once).anthropic().llm(M);
    let error = llm.ask("x").await.expect_err("exhausted");

    assert!(
        matches!(error, Error::RetriesExhausted { attempts: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Overloaded);
    assert_eq!(once.requests().len(), 1);

    let twice = overloaded_then_ok().await;
    let bedrock = Bedrock::builder("ABSK-x", REGION)
        .expect("a valid region")
        .base_url(twice.url())
        .expect("a valid URL")
        .sleeper(Arc::new(RecordingSleeper::default()))
        .build();
    bedrock.anthropic().llm(M).ask("x").await.expect("retried");
    assert_eq!(twice.requests().len(), 2);
}

/// P-16: a bearer token on Bedrock goes as `authorization` without the
/// OAuth beta header.
#[tokio::test]
async fn bearer_on_bedrock_sends_no_oauth_beta() {
    let stub = both_routes().await;
    let bearer = AuthMethod::Bearer("ABSK-x".into());

    let llm = bedrock_at(bearer, &stub).anthropic().llm(M);
    llm.ask("x").await.expect("ask");

    let requests = stub.requests();
    assert_eq!(header(&requests[0], "authorization"), Some("Bearer ABSK-x"));
    assert_eq!(header(&requests[0], "anthropic-beta"), None);
}

/// True when any header value of any request in `requests` contains
/// `secret`.
fn leaks(requests: &[RecordedRequest], secret: &str) -> bool {
    requests
        .iter()
        .any(|r| r.headers.values().any(|value| value.contains(secret)))
}

/// P-17: two endpoints with their own keys, used in turn, each send only
/// their own key.
#[tokio::test]
async fn keys_stay_with_their_endpoint() {
    let a = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", ANTHROPIC_SSE.to_string()),
    )])
    .await;
    let b = both_routes().await;
    let claude = Anthropic::builder("sk-ant-A")
        .base_url(a.url())
        .expect("a valid URL")
        .build();
    let (claude, bedrock) = (claude.llm(M), bedrock_at("ABSK-B", &b).anthropic().llm(M));

    for _ in 0..2 {
        claude.ask("x").await.expect("A");
        bedrock.ask("x").await.expect("B");
    }

    let (seen_a, seen_b) = (a.requests(), b.requests());
    assert_eq!((seen_a.len(), seen_b.len()), (2, 2));
    assert!(seen_a
        .iter()
        .all(|r| header(r, "x-api-key") == Some("sk-ant-A")));
    assert!(seen_b
        .iter()
        .all(|r| header(r, "x-api-key") == Some("ABSK-B")));
    assert!(!leaks(&seen_a, "ABSK-B"));
    assert!(!leaks(&seen_b, "sk-ant-A"));
}

/// The error `ask` ends with, and how long it took.
async fn timed(ask: impl Future<Output = Result<String, Error>>) -> (Error, Duration) {
    let started = Instant::now();
    let error = ask.await.expect_err("idle timeout");
    (error, started.elapsed())
}

/// A reply that reads the request and never answers.
fn silent() -> StubReply {
    StubReply {
        status: 200,
        content_type: "text/event-stream",
        headers: Vec::new(),
        body: StubBody::Hang,
    }
}

/// P-20: the endpoint's idle limit cuts off a silent server on both
/// protocols.
#[tokio::test]
async fn idle_timeout_belongs_to_the_endpoint() {
    let routes = vec![(MESSAGES, vec![silent()]), (CHAT, vec![silent()])];
    let stub = StubServer::start_replies(routes).await;
    let bedrock = Bedrock::builder("ABSK-x", REGION)
        .expect("a valid region")
        .base_url(stub.url())
        .expect("a valid URL")
        .timeouts(ProviderOptions::default().idle_gap(Duration::from_millis(200)))
        .retry(RetryPolicy::none())
        .build();
    let (claude, kimi) = (bedrock.anthropic().llm(M), bedrock.openai().llm(N));

    for (error, elapsed) in [timed(claude.ask("x")).await, timed(kimi.ask("y")).await] {
        let Error::RetriesExhausted { attempts, last } = &error else {
            panic!("expected RetriesExhausted, got {error:?}");
        };
        assert_eq!(*attempts, 1);
        assert!(
            matches!(**last, Error::IdleTimeout { after } if after == Duration::from_millis(200)),
            "{last:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Timeout);
        assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    }
}

/// P-22: Bedrock's 400 envelope reads as an `InvalidRequest` API error with
/// the server's message.
#[tokio::test]
async fn bedrock_error_envelope_is_an_api_error() {
    let body = concat!(
        r#"{"type":"error","error":{"type":"invalid_request_error","#,
        r#""message":"The provided model identifier is invalid."}}"#
    );
    let stub = StubServer::start(vec![(
        MESSAGES,
        (400, "application/json", body.to_string()),
    )])
    .await;

    let llm = bedrock_at("ABSK-x", &stub).anthropic().llm(M);
    let error = llm.ask("x").await.expect_err("400");

    let Error::Api(api) = &error else {
        panic!("expected Api, got {error:?}");
    };
    assert_eq!(api.kind(), ErrorKind::InvalidRequest);
    assert_eq!(api.message(), "The provided model identifier is invalid.");
}
