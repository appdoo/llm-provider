//! Tests for error classification: `Error::kind`, `is_retryable` and the
//! messages of `Api` and `Config`.

use std::time::Duration;

use super::*;

fn api(kind: ErrorKind, message: &str) -> Error {
    Error::Api(ApiError::new(kind, message))
}

/// `Api` Display names kind, status and message.
#[test]
fn api_variant_display() {
    let err = Error::Api(ApiError::new(ErrorKind::RateLimited, "slow down").with_status(429));

    assert_eq!(
        err.to_string(),
        "API error (RateLimited, status Some(429)): slow down"
    );
}

/// `kind()` per variant and `is_retryable()`.
#[test]
fn kind_per_variant_and_retryable() {
    let idle = Error::IdleTimeout {
        after: Duration::from_secs(1),
    };

    assert_eq!(idle.kind(), ErrorKind::Timeout);
    assert_eq!(idle.to_string(), "stream idle for 1s");

    let exhausted = Error::RetriesExhausted {
        attempts: 4,
        last: Box::new(api(ErrorKind::Overloaded, "o")),
    };

    assert_eq!(exhausted.kind(), ErrorKind::Overloaded);
    assert_eq!(
        exhausted.to_string(),
        "gave up after 4 attempts: API error (Overloaded, status None): o"
    );

    assert_eq!(
        Error::InvalidRequest(RequestError::EmptyBatch).kind(),
        ErrorKind::InvalidRequest
    );
    let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
    assert_eq!(Error::Json(json_err).kind(), ErrorKind::Other);
    let missing = |error: ConfigError| Error::Config(error).kind();
    assert_eq!(
        missing(ConfigError::MissingCredential { provider: "p" }),
        ErrorKind::Auth
    );
    assert_eq!(
        missing(ConfigError::MissingBaseUrl { provider: "p" }),
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        missing(ConfigError::MissingModel { provider: "p" }),
        ErrorKind::InvalidRequest
    );
    assert_eq!(Error::EmptyResponse.kind(), ErrorKind::Other);

    for (kind, retryable) in [
        (ErrorKind::RateLimited, true),
        (ErrorKind::Overloaded, true),
        (ErrorKind::Server, true),
        (ErrorKind::Timeout, true),
        (ErrorKind::Network, true),
        (ErrorKind::Auth, false),
        (ErrorKind::InvalidRequest, false),
        (ErrorKind::Other, false),
    ] {
        assert_eq!(api(kind, "m").is_retryable(), retryable, "{kind:?}");
    }
}

/// `Http` connection failure is `Network` (connect checked first).
#[tokio::test]
async fn http_connect_failure_is_network() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let err = reqwest::get(format!("http://127.0.0.1:{port}/"))
        .await
        .expect_err("nothing listens");
    let err = Error::Http(err);

    assert_eq!(err.kind(), ErrorKind::Network);
    assert!(err.is_retryable());
}
