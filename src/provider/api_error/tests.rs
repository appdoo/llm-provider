//! Tests for reading an `ApiError` from a response or an in-stream event.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::json;

use super::*;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_static(value));
    }

    map
}

fn envelope(error_type: &str) -> String {
    json!({"type": "error", "error": {"type": error_type, "message": "m"}}).to_string()
}

/// Request id, retry-after seconds, kind from the envelope.
#[test]
fn from_response_reads_headers() {
    let e = ApiError::from_response(
        429,
        &headers(&[("request-id", "req_123"), ("retry-after", "2")]),
        &envelope("rate_limit_error"),
    );
    assert_eq!(e.request_id(), Some("req_123"));
    assert_eq!(e.retry_after(), Some(Duration::from_secs(2)));
    assert_eq!(e.kind(), ErrorKind::RateLimited);
    assert_eq!(e.status(), Some(429));
    assert_eq!(e.message(), "m");

    let e = ApiError::from_response(
        429,
        &headers(&[("x-request-id", "req_9")]),
        &envelope("rate_limit_error"),
    );
    assert_eq!(e.request_id(), Some("req_9"));
    assert_eq!(e.retry_after(), None);

    let e = ApiError::from_response(
        429,
        &headers(&[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")]),
        &envelope("rate_limit_error"),
    );
    assert_eq!(e.retry_after(), None);
}

/// Every Anthropic `error.type` decides the kind, even at 200.
#[test]
fn envelope_type_decides_kind() {
    for (error_type, want) in [
        ("rate_limit_error", ErrorKind::RateLimited),
        ("overloaded_error", ErrorKind::Overloaded),
        ("api_error", ErrorKind::Server),
        ("authentication_error", ErrorKind::Auth),
        ("permission_error", ErrorKind::Auth),
        ("invalid_request_error", ErrorKind::InvalidRequest),
        ("not_found_error", ErrorKind::InvalidRequest),
        ("request_too_large", ErrorKind::InvalidRequest),
    ] {
        let e = ApiError::from_response(200, &HeaderMap::new(), &envelope(error_type));
        assert_eq!(e.kind(), want, "error.type {error_type}");
    }
}

/// Without an envelope the status decides the kind.
#[test]
fn status_decides_kind_without_envelope() {
    for (status, want) in [
        (429, ErrorKind::RateLimited),
        (529, ErrorKind::Overloaded),
        (500, ErrorKind::Server),
        (502, ErrorKind::Server),
        (503, ErrorKind::Server),
        (504, ErrorKind::Server),
        (408, ErrorKind::Timeout),
        (401, ErrorKind::Auth),
        (403, ErrorKind::Auth),
        (400, ErrorKind::InvalidRequest),
        (404, ErrorKind::InvalidRequest),
        (413, ErrorKind::InvalidRequest),
        (422, ErrorKind::InvalidRequest),
    ] {
        let e = ApiError::from_response(status, &HeaderMap::new(), "not json");
        assert_eq!(e.kind(), want, "status {status}");
        assert_eq!(e.message(), "not json");
    }
    let e = ApiError::from_response(418, &HeaderMap::new(), "teapot");
    assert_eq!(e.kind(), ErrorKind::Other);
    assert_eq!(e.message(), "teapot");
}

/// An OpenAI envelope gives the message; an unknown type falls back to the
/// status.
#[test]
fn openai_envelope_message_and_status_kind() {
    let body = json!({"error": {"message": "bad key", "type": "invalid_api_key", "code": null}})
        .to_string();
    let e = ApiError::from_response(401, &HeaderMap::new(), &body);
    assert_eq!(e.kind(), ErrorKind::Auth);
    assert_eq!(e.message(), "bad key");
}

/// In-stream error events: an Anthropic `error.type` decides the kind, an
/// OpenAI-compatible event is always `Other`.
#[test]
fn from_stream_event() {
    let e = ApiError::from_anthropic_event("overloaded_error", "Overloaded");
    assert_eq!(e.kind(), ErrorKind::Overloaded);
    assert_eq!(e.status(), None);
    assert_eq!(e.message(), "Overloaded");

    assert_eq!(
        ApiError::from_anthropic_event("weird_error", "x").kind(),
        ErrorKind::Other
    );

    let data =
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
    assert_eq!(ApiError::from_openai_event(&data).kind(), ErrorKind::Other);
    assert_eq!(ApiError::from_openai_event(&json!({})).message(), "");
}
