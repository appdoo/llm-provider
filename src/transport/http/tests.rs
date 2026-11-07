//! Tests for the shared HTTP client: the idle gap, redirects and error bodies.

use std::time::Duration;

use tokio_stream::StreamExt;

use super::Http;
use crate::provider::test_stub::{StubBody, StubReply, StubServer};
use crate::provider::{Error, ErrorKind, ProviderOptions};

/// A pool with a 1 s idle gap.
fn one_second_idle() -> Http {
    Http::new(ProviderOptions::default().idle_gap(Duration::from_secs(1)))
}

/// A stub answering one POST to `/` with `status` and `body`.
async fn stub(status: u16, body: StubBody) -> StubServer {
    StubServer::start_replies(vec![(
        ("POST", "/"),
        vec![StubReply {
            status,
            content_type: "text/event-stream",
            headers: Vec::new(),
            body,
        }],
    )])
    .await
}

/// True when `error` is the idle gap of 1 s running out.
fn is_one_second_idle(error: &Error) -> bool {
    matches!(error, Error::IdleTimeout { after } if *after == Duration::from_secs(1))
}

/// No headers at all: `IdleTimeout` after 1 s, from reqwest's read timeout.
#[tokio::test]
async fn idle_gap_before_headers() {
    let (http, stub) = (one_second_idle(), stub(200, StubBody::Hang).await);
    let started = std::time::Instant::now();

    let request = http.client().await.expect("client").post(stub.url());
    let err = http.send(request).await.expect_err("must time out");

    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(is_one_second_idle(&err), "got {err:?}");
}

/// Headers, then silence: the byte stream ends in `IdleTimeout` after 1 s.
#[tokio::test]
async fn idle_gap_between_chunks() {
    let (http, stub) = (
        one_second_idle(),
        stub(200, StubBody::HeadersThenHang).await,
    );
    let started = std::time::Instant::now();

    let request = http.client().await.expect("client").post(stub.url());
    let bytes = http.byte_stream(request).await.expect("headers arrive");
    let items: Vec<_> = bytes.collect().await;

    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(items.len(), 1, "items: {items:?}");
    assert!(
        matches!(&items[0], Err(error) if is_one_second_idle(error)),
        "got {:?}",
        items[0]
    );
}

/// A 302 is not followed: `Api` with status 302, the target is never hit.
#[tokio::test]
async fn redirect_is_not_followed() {
    let target = StubServer::start(vec![(("POST", "/"), (200, "text/plain", "ok".into()))]).await;
    let origin = StubServer::start_replies(vec![(
        ("POST", "/"),
        vec![StubReply {
            status: 302,
            content_type: "text/plain",
            headers: vec![("location", format!("{}/", target.url()))],
            body: StubBody::Full(String::new()),
        }],
    )])
    .await;
    let http = Http::new(ProviderOptions::default());

    let request = http.client().await.expect("client").post(origin.url());
    let err = http.send(request).await.expect_err("302");

    assert_eq!(err.kind(), ErrorKind::Other);
    match err {
        Error::Api(e) => assert_eq!(e.status(), Some(302)),
        other => panic!("expected Api, got {other:?}"),
    }
    assert_eq!(origin.requests().len(), 1);
    assert_eq!(target.requests().len(), 0);
}

/// A 200 KiB error body is cut to the 64 KiB cap.
#[tokio::test]
async fn error_body_is_capped() {
    let stub = stub(500, StubBody::Full("a".repeat(200 * 1024))).await;
    let http = Http::new(ProviderOptions::default());

    let request = http.client().await.expect("client").post(stub.url());

    match http.send(request).await.expect_err("500") {
        Error::Api(e) => {
            assert_eq!(e.status(), Some(500));
            assert_eq!(e.message(), "a".repeat(65_536));
        }
        other => panic!("expected Api, got {other:?}"),
    }
}

/// An error body that never arrives ends in `IdleTimeout`.
#[tokio::test]
async fn error_body_idle_timeout() {
    let (http, stub) = (
        one_second_idle(),
        stub(500, StubBody::HeadersThenHang).await,
    );
    let started = std::time::Instant::now();

    let request = http.client().await.expect("client").post(stub.url());
    let err = http.send(request).await.expect_err("hang");

    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(is_one_second_idle(&err), "got {err:?}");
}
