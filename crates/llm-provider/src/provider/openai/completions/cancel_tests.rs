//! Tests for cancelling the `/completions` path: a dropped stream closes the request,
//! and `complete_cancellable` returns the partial text, sends nothing on a
//! cancelled token, never retries after a cancel and lets `MessageStop` win.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{sleep, timeout};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::provider::completion::{fold_cancellable, Completion};
use crate::provider::error::Error;
use crate::provider::test_stub::{BlockingSleeper, StubBody, StubReply, StubServer};
use crate::provider::{
    EventStream, OpenAi, RawLlm, RawRequest, RetryPolicy, StopReason, StreamEvent,
};

/// The route every raw request goes to.
const COMPLETIONS: (&str, &str) = ("POST", "/v1/completions");

/// How long a test waits for the stub to see the client close.
const CLOSE_WITHIN: Duration = Duration::from_secs(2);

/// The two raw chunks `"ab"` and `"c"` as event-stream lines, no stop.
fn ab_c() -> Vec<String> {
    [
        r#"{"choices":[{"text":"ab","finish_reason":null}]}"#,
        r#"{"choices":[{"text":"c","finish_reason":null}]}"#,
    ]
    .iter()
    .map(|chunk| format!("data: {chunk}\n\n"))
    .collect()
}

/// A `200 text/event-stream` reply with `body`.
fn reply(body: StubBody) -> StubReply {
    StubReply {
        status: 200,
        content_type: "text/event-stream",
        headers: Vec::new(),
        body,
    }
}

/// A stub answering every raw request with `body`.
async fn stub(body: StubBody) -> StubServer {
    StubServer::start_replies(vec![(COMPLETIONS, vec![reply(body)])]).await
}

/// The model `m` on the stub's `/v1` root.
fn model(stub: &StubServer) -> RawLlm {
    OpenAi::at(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .raw("m")
}

/// The partial text of an `Interrupted` error.
fn partial_text(result: Result<Completion, Error>) -> String {
    match result {
        Err(Error::Interrupted { partial_text, .. }) => partial_text,
        other => panic!("expected Interrupted, got {other:?}"),
    }
}

/// B-1: reading two events and dropping the stream closes the request.
#[tokio::test]
async fn dropped_raw_stream_closes_the_request() {
    let stub = stub(StubBody::Events { lines: ab_c() }).await;
    let mut stream = model(&stub).stream(RawRequest::new("x"));

    for _ in 0..2 {
        stream.next().await.expect("an event").expect("no error");
    }
    drop(stream);

    assert!(timeout(CLOSE_WITHIN, stub.closed()).await.is_ok());
}

/// B-3: a stream dropped before the first byte closes the request.
#[tokio::test]
async fn raw_stream_dropped_before_first_byte_closes_the_request() {
    let stub = stub(StubBody::Hang).await;
    let mut stream = model(&stub).stream(RawRequest::new("x"));

    let polled = timeout(Duration::from_millis(100), stream.next()).await;
    drop(stream);

    assert!(polled.is_err(), "the stub sends nothing");
    assert!(timeout(CLOSE_WITHIN, stub.closed()).await.is_ok());
}

/// B-4: a token fired after `"ab"`, `"c"` returns `Interrupted("abc")` and
/// closes the request.
#[tokio::test]
async fn token_cancel_returns_the_partial_text() {
    let stub = stub(StubBody::Events { lines: ab_c() }).await;
    let model = model(&stub);
    let token = CancellationToken::new();

    let cancel = async {
        stub.written(2).await;
        sleep(Duration::from_millis(50)).await;
        token.cancel();
    };
    let (result, ()) = tokio::join!(
        model.complete_cancellable(RawRequest::new("x"), &token),
        cancel
    );

    assert_eq!(partial_text(result), "abc");
    assert!(timeout(CLOSE_WITHIN, stub.closed()).await.is_ok());
}

/// B-5: a cancel during the retry backoff after a 529 returns
/// `Interrupted("")` with one request sent.
#[tokio::test]
async fn cancel_in_backoff_sends_no_retry() {
    let overloaded = StubReply::full(529, "application/json", r#"{"error":{"message":"busy"}}"#);
    let stub = StubServer::start_replies(vec![(COMPLETIONS, vec![overloaded])]).await;
    let sleeper = Arc::new(BlockingSleeper::default());
    let model = OpenAi::builder(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .retry(RetryPolicy::default().max_attempts(3))
        .sleeper(sleeper.clone())
        .build()
        .raw("m");
    let token = CancellationToken::new();

    let cancel = async {
        sleeper.entered().await;
        token.cancel();
    };
    let (result, ()) = tokio::join!(
        model.complete_cancellable(RawRequest::new("x"), &token),
        cancel
    );

    assert_eq!(partial_text(result), "");
    assert_eq!(stub.requests().len(), 1);
}

/// B-6: a token cancelled before the call returns `Interrupted("")` and
/// sends no request.
#[tokio::test]
async fn cancelled_token_sends_nothing() {
    let stub = stub(StubBody::Events { lines: ab_c() }).await;
    let token = CancellationToken::new();
    token.cancel();

    let result = model(&stub)
        .complete_cancellable(RawRequest::new("x"), &token)
        .await;

    assert_eq!(partial_text(result), "");
    assert_eq!(stub.requests().len(), 0);
}

/// B-7: a `MessageStop` ready together with the cancel wins: `Ok("ab")`.
/// (Adapted: `RawProvider` is gone, so the stream goes to the fold directly.)
#[tokio::test]
async fn message_stop_wins_over_a_simultaneous_cancel() {
    let token = CancellationToken::new();
    let fired = token.clone();
    let events: EventStream = Box::pin(async_stream::stream! {
        yield Ok(StreamEvent::ContentDelta("ab".into()));
        fired.cancel();
        yield Ok(StreamEvent::Stop(StopReason::EndTurn));
        yield Ok(StreamEvent::MessageStop);
    });

    let completion = fold_cancellable(events, |_| {}, &token)
        .await
        .expect("MessageStop wins");

    assert_eq!(completion.text, "ab");
}
