//! Tests for the public client: `ask` and the request builder through the
//! retry policy, the credential headers, a caller's HTTP client and `Debug`.

use std::time::Duration;

use super::Anthropic;
use crate::provider::anthropic::test_support::TEXT_SSE;
use crate::provider::auth::AuthMethod;
use crate::provider::error::{Error, ErrorKind};
use crate::provider::retry::RetryPolicy;
use crate::provider::test_stub::{StubReply, StubServer};

async fn front_stub() -> StubServer {
    StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", TEXT_SSE.to_string()),
    )])
    .await
}

fn body_of(stub: &StubServer, i: usize) -> serde_json::Value {
    serde_json::from_str(&stub.requests()[i].body).expect("json body")
}

/// `ask` returns the text and sends key, model, default limit, user message.
#[tokio::test]
async fn ask_sends_key_and_defaults() {
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

/// the builder's system, model and limit reach the wire.
#[tokio::test]
async fn request_builder_overrides() {
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
            StubReply::full(200, "text/event-stream", TEXT_SSE),
        ],
    )]
}

/// retry is on by default-shaped policy; a 529 is retried.
#[tokio::test]
async fn retries_overloaded() {
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

/// `RetryPolicy::none()` gives up after the first attempt.
#[tokio::test]
async fn retry_none_gives_up() {
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
    assert_eq!(err.kind(), ErrorKind::Overloaded);
    assert_eq!(stub.requests().len(), 1);
}

/// a Bearer credential sends OAuth headers and no `x-api-key`.
#[tokio::test]
async fn bearer_sends_oauth_headers() {
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

/// A caller's HTTP client sends the requests: its user agent reaches the
/// server.
#[tokio::test]
async fn http_client_sends_requests() {
    let stub = front_stub().await;
    let client = reqwest::Client::builder()
        .user_agent("probe")
        .build()
        .expect("client");
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .http_client(client)
        .build();

    assert_eq!(claude.ask("Hello").await.expect("ask"), "Hi");

    let headers = &stub.requests()[0].headers;

    assert_eq!(headers.get("user-agent").map(String::as_str), Some("probe"));
}

/// The default policy makes four attempts; `none()` makes one.
#[test]
fn policy_defaults() {
    assert_eq!(RetryPolicy::default(), RetryPolicy::none().max_attempts(4));
    assert_eq!(RetryPolicy::none(), RetryPolicy::default().max_attempts(1));
    assert_eq!(Anthropic::new("k", "m").policy(), RetryPolicy::default());
}

/// `Debug` shows the model and never the credential.
#[test]
fn debug_hides_credential() {
    let shown = format!("{:?}", Anthropic::new("sk-secret", "m"));

    assert!(!shown.contains("sk-secret"), "{shown}");
    assert!(shown.contains("\"m\""), "{shown}");
}
