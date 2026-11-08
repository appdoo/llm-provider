//! Tests for retry: decisions, backoff, `Retry-After`, the streaming retry window and batch retries.

use std::sync::Arc;
use std::time::Duration;

use tokio_stream::StreamExt;

use super::run::next_wait;
use super::RetryPolicy;
use crate::provider::error::{Error, ErrorKind};
use crate::provider::test_stub::{RecordingSleeper, StubReply, StubServer};
use crate::provider::Anthropic;
use crate::provider::ApiError;
use crate::provider::StreamEvent;
use crate::provider::{CompletionRequest, LlmProvider};
use crate::provider::{Content, Message, MessageRole, ModelSelection};

const TEXT_SSE: &str = include_str!("../../../tests/fixtures/anthropic_text.sse");
const RATE_LIMIT: &str =
    r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
const OVERLOADED: &str =
    r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
const INVALID: &str =
    r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"}}"#;
const MESSAGES: (&str, &str) = ("POST", "/v1/messages");

fn json(status: u16, body: &str) -> StubReply {
    StubReply::full(status, "application/json", body)
}

fn sse(body: &str) -> StubReply {
    StubReply::full(200, "text/event-stream", body)
}

fn with_headers(mut reply: StubReply, headers: &[(&'static str, &str)]) -> StubReply {
    reply.headers = headers
        .iter()
        .map(|(k, v)| (*k, (*v).to_string()))
        .collect();
    reply
}

fn in_stream_overloaded() -> String {
    format!("event: error\ndata: {OVERLOADED}\n\n")
}

/// A stub answering `replies` in turn and a client with `policy` that
/// records its waits instead of sleeping.
async fn setup(
    replies: Vec<StubReply>,
    policy: RetryPolicy,
) -> (StubServer, Anthropic, Arc<RecordingSleeper>) {
    let stub = StubServer::start_replies(vec![(MESSAGES, replies)]).await;
    let recorder = Arc::new(RecordingSleeper::default());
    let provider = Anthropic::builder("test-key", "m")
        .base_url(stub.url())
        .retry(policy)
        .build()
        .with_sleeper(recorder.clone());
    (stub, provider, recorder)
}

fn request() -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: "hello".to_string(),
            }],
        }]),
        tools: vec![],
        max_tokens: 16,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    }
}

fn api_of(e: &Error) -> &ApiError {
    match e {
        Error::Api(a) => a,
        other => panic!("expected Api, got {other:?}"),
    }
}

/// 429 with `retry-after: 2`, then 200: one sleep of exactly 2 s.
#[tokio::test]
async fn retry_after_is_honoured() {
    let first = with_headers(json(429, RATE_LIMIT), &[("retry-after", "2")]);
    let (stub, provider, recorder) =
        setup(vec![first, sse(TEXT_SSE)], RetryPolicy::default()).await;
    let completion = provider.complete(request()).await.expect("complete");

    assert_eq!(completion.text, "Hi");
    assert_eq!(stub.requests().len(), 2);
    assert_eq!(recorder.slept(), vec![Duration::from_secs(2)]);
}

/// 529 then 200: one jittered sleep in 0.5 s..=1 s.
#[tokio::test]
async fn backoff_with_jitter() {
    let (stub, provider, recorder) = setup(
        vec![json(529, OVERLOADED), sse(TEXT_SSE)],
        RetryPolicy::default(),
    )
    .await;
    provider.complete(request()).await.expect("complete");

    assert_eq!(stub.requests().len(), 2);
    let slept = recorder.slept();
    assert_eq!(slept.len(), 1);
    assert!(
        slept[0] >= Duration::from_millis(500) && slept[0] <= Duration::from_secs(1),
        "sleep out of range: {:?}",
        slept[0]
    );
}

/// 400 invalid request is not retried.
#[tokio::test]
async fn invalid_request_not_retried() {
    let (stub, provider, recorder) = setup(vec![json(400, INVALID)], RetryPolicy::default()).await;
    let err = provider.complete(request()).await.expect_err("must fail");

    assert_eq!(api_of(&err).kind(), ErrorKind::InvalidRequest);
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(recorder.slept(), Vec::<Duration>::new());
}

/// 401 is not retried.
#[tokio::test]
async fn auth_not_retried() {
    let (stub, provider, recorder) = setup(
        vec![StubReply::full(401, "text/plain", "no")],
        RetryPolicy::default(),
    )
    .await;
    let err = provider.complete(request()).await.expect_err("must fail");

    assert!(matches!(err, Error::Api(_)), "got {err:?}");
    assert_eq!(err.kind(), ErrorKind::Auth);
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(recorder.slept(), Vec::<Duration>::new());
}

/// 529 on every request: 4 attempts, 3 sleeps, then exhausted.
#[tokio::test]
async fn exhausted_after_four_attempts() {
    let (stub, provider, recorder) =
        setup(vec![json(529, OVERLOADED)], RetryPolicy::default()).await;
    let err = provider.complete(request()).await.expect_err("must fail");

    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 4, .. }),
        "got {err:?}"
    );
    assert_eq!(err.kind(), ErrorKind::Overloaded);
    assert_eq!(stub.requests().len(), 4);
    assert_eq!(recorder.slept().len(), 3);
}

/// `retry-after` beyond the cap gives up at once.
#[tokio::test]
async fn retry_after_over_cap_gives_up() {
    let first = with_headers(json(429, RATE_LIMIT), &[("retry-after", "120")]);
    let (stub, provider, recorder) = setup(vec![first], RetryPolicy::default()).await;
    let err = provider.complete(request()).await.expect_err("must fail");

    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
        "got {err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(recorder.slept(), Vec::<Duration>::new());
}

/// The cap also applies on a later attempt, while the schedule still has
/// waits left.
#[test]
fn cap_on_later_attempt() {
    let e = Error::Api(
        ApiError::new(ErrorKind::RateLimited, "slow down")
            .with_status(429)
            .with_retry_after(Duration::from_secs(120)),
    );
    let mut backoff = RetryPolicy::default().backoff();
    let _first = backoff.next();

    assert_eq!(next_wait(&RetryPolicy::default(), &e, backoff.next()), None);
}

/// An error after only `message_start` is retried; usage of the failed
/// attempt is dropped, so input tokens count once.
#[tokio::test]
async fn retry_window_drops_buffered_usage() {
    let start = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n";
    let first = sse(&format!("{start}{}", in_stream_overloaded()));
    let (stub, provider, _recorder) =
        setup(vec![first, sse(TEXT_SSE)], RetryPolicy::default()).await;
    let completion = provider.complete(request()).await.expect("complete");

    assert_eq!(stub.requests().len(), 2);
    assert_eq!(completion.usage.input_tokens, 10);
}

/// After content arrived, an error is forwarded and never retried.
#[tokio::test]
async fn no_retry_after_content() {
    let delta = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n";
    let body = format!("{delta}{}", in_stream_overloaded());
    let (stub, provider, _recorder) = setup(vec![sse(&body)], RetryPolicy::default()).await;
    let items: Vec<_> = provider.stream(request()).collect().await;

    assert_eq!(items.len(), 2, "items: {items:?}");
    assert!(
        matches!(&items[0], Ok(StreamEvent::ContentDelta(t)) if t == "Hi"),
        "first: {:?}",
        items[0]
    );
    match &items[1] {
        Err(e) => {
            let a = api_of(e);
            assert_eq!(a.kind(), ErrorKind::Overloaded);
            assert_eq!(a.status(), None);
        }
        other => panic!("expected Err, got {other:?}"),
    }
    assert_eq!(stub.requests().len(), 1);
}

/// `max_attempts(1)` turns retry off.
#[tokio::test]
async fn one_attempt_is_retry_off() {
    let policy = RetryPolicy::default().max_attempts(1);
    let (stub, provider, _recorder) =
        setup(vec![json(529, OVERLOADED), sse(TEXT_SSE)], policy).await;
    let err = provider.complete(request()).await.expect_err("must fail");

    assert!(
        matches!(err, Error::RetriesExhausted { attempts: 1, .. }),
        "got {err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
}

/// The request id survives into `RetriesExhausted.last`.
#[tokio::test]
async fn request_id_end_to_end() {
    let policy = RetryPolicy::default().max_attempts(1);
    let reply = with_headers(
        json(429, RATE_LIMIT),
        &[("request-id", "req_123"), ("retry-after", "2")],
    );
    let (_stub, provider, _recorder) = setup(vec![reply], policy).await;
    match provider.complete(request()).await {
        Err(Error::RetriesExhausted { last, .. }) => {
            assert_eq!(api_of(&last).request_id(), Some("req_123"));
        }
        other => panic!("expected RetriesExhausted, got {other:?}"),
    }
}
