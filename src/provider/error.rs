//! Provider error types.

use std::time::Duration;

/// Classification of a provider failure. Callers branch on this, never on
/// message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    RateLimited,
    Overloaded,
    Server,
    Timeout,
    Network,
    Auth,
    InvalidRequest,
    Other,
}

/// An error answer from a provider API, as an HTTP response or an in-stream
/// error event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub kind: ErrorKind,
    /// `None` for an in-stream error event or a batch result.
    pub status: Option<u16>,
    /// Envelope message, or the raw body when there is no envelope.
    pub message: String,
    /// Header `request-id` (Anthropic) or `x-request-id` (OpenAI).
    pub request_id: Option<String>,
    /// Header `retry-after`, integer seconds only.
    pub retry_after: Option<Duration>,
}

/// Kind from an Anthropic `error.type`, if the type is a known one.
fn kind_from_type(error_type: &str) -> Option<ErrorKind> {
    match error_type {
        "rate_limit_error" => Some(ErrorKind::RateLimited),
        "overloaded_error" => Some(ErrorKind::Overloaded),
        "api_error" => Some(ErrorKind::Server),
        "authentication_error" | "permission_error" => Some(ErrorKind::Auth),
        "invalid_request_error" | "not_found_error" | "request_too_large" => {
            Some(ErrorKind::InvalidRequest)
        }
        _ => None,
    }
}

/// Kind from an HTTP status.
fn kind_from_status(status: u16) -> ErrorKind {
    match status {
        429 => ErrorKind::RateLimited,
        529 => ErrorKind::Overloaded,
        500 | 502 | 503 | 504 => ErrorKind::Server,
        408 => ErrorKind::Timeout,
        401 | 403 => ErrorKind::Auth,
        400 | 404 | 413 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Other,
    }
}

impl ApiError {
    /// Build from a non-2xx HTTP response. Together with
    /// `from_stream_event`, the only code that reads an error body.
    pub(crate) fn from_response(
        status: u16,
        headers: &reqwest::header::HeaderMap,
        body: &str,
    ) -> ApiError {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let request_id = header("request-id").or_else(|| header("x-request-id"));
        let retry_after = header("retry-after")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let envelope: Option<serde_json::Value> = serde_json::from_str(body).ok();
        let error = envelope.as_ref().and_then(|v| v.get("error"));
        let message = error
            .and_then(|e| e.get("message"))
            .and_then(serde_json::Value::as_str)
            .map_or_else(|| body.to_string(), str::to_string);
        let kind = error
            .and_then(|e| e.get("type"))
            .and_then(serde_json::Value::as_str)
            .and_then(kind_from_type)
            .unwrap_or_else(|| kind_from_status(status));
        ApiError {
            kind,
            status: Some(status),
            message,
            request_id,
            retry_after,
        }
    }

    /// Build from an in-stream error event. Only an Anthropic event carries
    /// a typed `error.type`; an OpenAI in-stream error is always `Other`.
    pub(crate) fn from_stream_event(data: &serde_json::Value, anthropic: bool) -> ApiError {
        let error = &data["error"];
        let kind = if anthropic {
            error["type"]
                .as_str()
                .and_then(kind_from_type)
                .unwrap_or(ErrorKind::Other)
        } else {
            ErrorKind::Other
        };
        ApiError {
            kind,
            status: None,
            message: error["message"].as_str().unwrap_or_default().to_string(),
            request_id: None,
            retry_after: None,
        }
    }
}

/// Errors that can occur in the provider layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    #[error("Missing credentials: {message}")]
    MissingCredentials { message: String },
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SSE parse error: {message}")]
    SseParse { message: String },
    #[error("API error ({:?}, status {:?}): {}", .0.kind, .0.status, .0.message)]
    Api(ApiError),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// The provider factory received a name it does not recognise.
    #[error("Unknown provider: {0}")]
    UnknownProvider(String),
    /// The request was rejected locally before any network I/O,
    /// e.g. invalid cache breakpoints.
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
    /// No bytes arrived on an open stream for `after`.
    #[error("stream idle for {after:?}")]
    IdleTimeout { after: Duration },
    /// Retries ran out; `last` is the final attempt's error.
    #[error("gave up after {attempts} attempts: {last}")]
    RetriesExhausted {
        attempts: u32,
        last: Box<ProviderError>,
    },
}

impl ProviderError {
    /// The typed classification of this error.
    pub fn kind(&self) -> ErrorKind {
        match self {
            ProviderError::Api(e) => e.kind,
            ProviderError::IdleTimeout { .. } => ErrorKind::Timeout,
            ProviderError::Http(e) => {
                if e.is_connect() {
                    ErrorKind::Network
                } else if e.is_timeout() {
                    ErrorKind::Timeout
                } else if e.is_request() || e.is_body() {
                    ErrorKind::Network
                } else {
                    ErrorKind::Other
                }
            }
            ProviderError::Auth(_) | ProviderError::MissingCredentials { .. } => ErrorKind::Auth,
            ProviderError::InvalidRequest(_) => ErrorKind::InvalidRequest,
            ProviderError::Json(_)
            | ProviderError::SseParse { .. }
            | ProviderError::Io(_)
            | ProviderError::UnknownProvider(_) => ErrorKind::Other,
            ProviderError::RetriesExhausted { last, .. } => last.kind(),
        }
    }

    /// True when another attempt may succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind(),
            ErrorKind::RateLimited
                | ErrorKind::Overloaded
                | ErrorKind::Server
                | ErrorKind::Timeout
                | ErrorKind::Network
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::json;

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

    fn api(kind: ErrorKind, message: &str) -> ProviderError {
        ProviderError::Api(ApiError {
            kind,
            status: None,
            message: message.to_string(),
            request_id: None,
            retry_after: None,
        })
    }

    /// `Api` Display names kind, status and message.
    #[test]
    fn api_variant_display() {
        let err = ProviderError::Api(ApiError {
            kind: ErrorKind::RateLimited,
            status: Some(429),
            message: "slow down".to_string(),
            request_id: None,
            retry_after: None,
        });
        assert_eq!(
            err.to_string(),
            "API error (RateLimited, status Some(429)): slow down"
        );
    }

    /// Request id, retry-after seconds, kind from the envelope.
    #[test]
    fn from_response_reads_headers() {
        let e = ApiError::from_response(
            429,
            &headers(&[("request-id", "req_123"), ("retry-after", "2")]),
            &envelope("rate_limit_error"),
        );
        assert_eq!(e.request_id.as_deref(), Some("req_123"));
        assert_eq!(e.retry_after, Some(Duration::from_secs(2)));
        assert_eq!(e.kind, ErrorKind::RateLimited);
        assert_eq!(e.status, Some(429));
        assert_eq!(e.message, "m");

        let e = ApiError::from_response(
            429,
            &headers(&[("x-request-id", "req_9")]),
            &envelope("rate_limit_error"),
        );
        assert_eq!(e.request_id.as_deref(), Some("req_9"));
        assert_eq!(e.retry_after, None);

        let e = ApiError::from_response(
            429,
            &headers(&[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")]),
            &envelope("rate_limit_error"),
        );
        assert_eq!(e.retry_after, None);
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
            assert_eq!(e.kind, want, "error.type {error_type}");
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
            assert_eq!(e.kind, want, "status {status}");
            assert_eq!(e.message, "not json");
        }
        let e = ApiError::from_response(418, &HeaderMap::new(), "teapot");
        assert_eq!(e.kind, ErrorKind::Other);
        assert_eq!(e.message, "teapot");
    }

    /// An OpenAI envelope gives the message; an unknown type falls
    /// back to the status.
    #[test]
    fn openai_envelope_message_and_status_kind() {
        let body =
            json!({"error": {"message": "bad key", "type": "invalid_api_key", "code": null}})
                .to_string();
        let e = ApiError::from_response(401, &HeaderMap::new(), &body);
        assert_eq!(e.kind, ErrorKind::Auth);
        assert_eq!(e.message, "bad key");
    }

    /// In-stream error events.
    #[test]
    fn from_stream_event() {
        let data = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        let e = ApiError::from_stream_event(&data, true);
        assert_eq!(e.kind, ErrorKind::Overloaded);
        assert_eq!(e.status, None);
        assert_eq!(e.message, "Overloaded");

        let unknown = json!({"error": {"type": "weird_error", "message": "x"}});
        assert_eq!(
            ApiError::from_stream_event(&unknown, true).kind,
            ErrorKind::Other
        );

        assert_eq!(
            ApiError::from_stream_event(&data, false).kind,
            ErrorKind::Other
        );
        assert_eq!(ApiError::from_stream_event(&json!({}), true).message, "");
    }

    /// `kind()` per variant and `is_retryable()`.
    #[test]
    fn kind_per_variant_and_retryable() {
        let idle = ProviderError::IdleTimeout {
            after: Duration::from_secs(1),
        };
        assert_eq!(idle.kind(), ErrorKind::Timeout);
        assert_eq!(idle.to_string(), "stream idle for 1s");

        let exhausted = ProviderError::RetriesExhausted {
            attempts: 4,
            last: Box::new(api(ErrorKind::Overloaded, "o")),
        };
        assert_eq!(exhausted.kind(), ErrorKind::Overloaded);
        assert_eq!(
            exhausted.to_string(),
            "gave up after 4 attempts: API error (Overloaded, status None): o"
        );

        assert_eq!(
            ProviderError::InvalidRequest("x".to_string()).kind(),
            ErrorKind::InvalidRequest
        );
        let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        assert_eq!(ProviderError::Json(json_err).kind(), ErrorKind::Other);
        assert_eq!(ProviderError::Auth("a".into()).kind(), ErrorKind::Auth);
        assert_eq!(
            ProviderError::MissingCredentials {
                message: "m".into()
            }
            .kind(),
            ErrorKind::Auth
        );
        assert_eq!(
            ProviderError::SseParse {
                message: "m".into()
            }
            .kind(),
            ErrorKind::Other
        );
        assert_eq!(
            ProviderError::UnknownProvider("p".into()).kind(),
            ErrorKind::Other
        );

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
        let err = ProviderError::Http(err);
        assert_eq!(err.kind(), ErrorKind::Network);
        assert!(err.is_retryable());
    }
}
