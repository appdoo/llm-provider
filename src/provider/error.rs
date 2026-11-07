//! The provider layer's errors: [`Error`] for every failure, [`ApiError`] for
//! an error answer from a provider API and [`RequestError`] for a request
//! rejected before any network I/O.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::provider::batch::BatchId;
use crate::provider::traits::BreakpointPosition;

/// The class of a provider failure. Callers branch on it, never on message
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The provider throttled the caller (HTTP 429, `rate_limit_error`).
    RateLimited,
    /// The provider is temporarily overloaded (HTTP 529, `overloaded_error`).
    Overloaded,
    /// The provider failed internally (HTTP 5xx, `api_error`).
    Server,
    /// A request or a response stream ran out of time.
    Timeout,
    /// The connection failed or broke.
    Network,
    /// The credential was missing, invalid or not allowed.
    Auth,
    /// The request was malformed or rejected as such.
    InvalidRequest,
    /// Anything else.
    Other,
}

/// An error answer from a provider API, as an HTTP response or an in-stream
/// error event.
///
/// Its fields stay public: callers outside the provider layer (the agent
/// loop's tests among them) build it by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// The class of the failure.
    pub kind: ErrorKind,
    /// The HTTP status; `None` for an in-stream error event or a batch result.
    pub status: Option<u16>,
    /// The envelope's message, or the raw body when there is no envelope.
    pub message: String,
    /// Header `request-id` (Anthropic) or `x-request-id` (OpenAI).
    pub request_id: Option<String>,
    /// Header `retry-after`, integer seconds only.
    pub retry_after: Option<Duration>,
}

impl ApiError {
    /// The error carried by a non-2xx HTTP response: the kind comes from the
    /// envelope's `error.type` when it is a known one, else from `status`.
    pub(crate) fn from_response(
        status: u16,
        headers: &reqwest::header::HeaderMap,
        body: &str,
    ) -> ApiError {
        let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        let request_id = header("request-id")
            .or_else(|| header("x-request-id"))
            .map(str::to_string);
        let retry_after = header("retry-after")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let error = serde_json::from_str::<Envelope>(body)
            .ok()
            .and_then(|envelope| envelope.error);
        let kind = error
            .as_ref()
            .and_then(|e| e.kind.as_deref())
            .and_then(kind_from_type)
            .unwrap_or_else(|| kind_from_status(status));
        let message = error
            .and_then(|e| e.message)
            .unwrap_or_else(|| body.to_string());
        ApiError {
            kind,
            status: Some(status),
            message,
            request_id,
            retry_after,
        }
    }

    /// The error carried by an Anthropic in-stream `error` event or an
    /// errored batch result, from its `error.type` and `error.message`.
    pub(crate) fn from_anthropic_event(error_type: &str, message: impl Into<String>) -> ApiError {
        ApiError {
            kind: kind_from_type(error_type).unwrap_or(ErrorKind::Other),
            status: None,
            message: message.into(),
            request_id: None,
            retry_after: None,
        }
    }

    /// The error carried by an OpenAI-compatible in-stream `error` chunk;
    /// its kind is always [`ErrorKind::Other`].
    pub(crate) fn from_openai_event(data: &serde_json::Value) -> ApiError {
        ApiError {
            kind: ErrorKind::Other,
            status: None,
            message: data["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            request_id: None,
            retry_after: None,
        }
    }
}

/// The envelope of an error body: `{"error": {"type": …, "message": …}}`,
/// as Anthropic and OpenAI-compatible servers write it.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    error: Option<EnvelopeError>,
}

/// The `error` object of an [`Envelope`].
#[derive(Debug, Deserialize)]
struct EnvelopeError {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// The kind named by an Anthropic `error.type`, if it is a known one.
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

/// The kind an HTTP status stands for.
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

/// Which bounded buffer of an event stream overflowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Oversize {
    /// One line of an event stream.
    Line,
    /// The `data:` payload of one server-sent event.
    Event,
    /// The streamed input of one tool call.
    ToolInput,
}

impl fmt::Display for Oversize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Oversize::Line => "event-stream line",
            Oversize::Event => "event payload",
            Oversize::ToolInput => "tool input",
        })
    }
}

/// A request rejected locally, before any network I/O.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RequestError {
    /// The request carries more cache breakpoints than the API accepts.
    #[error("too many cache breakpoints: {count} (max {max})")]
    TooManyBreakpoints {
        /// Breakpoints in the request.
        count: usize,
        /// Breakpoints the API accepts.
        max: usize,
    },
    /// A breakpoint names a message index past the last message.
    #[error("cache breakpoint Message({index}) out of range")]
    BreakpointOutOfRange {
        /// The message index the breakpoint names.
        index: usize,
    },
    /// A breakpoint names a message without content blocks.
    #[error("cache breakpoint Message({index}) points at an empty message")]
    BreakpointOnEmptyMessage {
        /// The message index the breakpoint names.
        index: usize,
    },
    /// A `System` breakpoint in a request without a system prompt.
    #[error("cache breakpoint System without a system prompt")]
    BreakpointWithoutSystem,
    /// A `Tools` breakpoint in a request without tools.
    #[error("cache breakpoint Tools without tools")]
    BreakpointWithoutTools,
    /// Two breakpoints name the same position.
    #[error("duplicate cache breakpoint {0:?}")]
    DuplicateBreakpoint(BreakpointPosition),
    /// A one-hour breakpoint follows a five-minute one in the order tools,
    /// system, messages.
    #[error("1h cache breakpoint after a 5m breakpoint")]
    TtlOrder,
    /// A batch without requests.
    #[error("batch has no requests")]
    EmptyBatch,
    /// A batch `custom_id` that does not match `^[a-zA-Z0-9_-]{1,64}$`.
    #[error("invalid custom_id {0:?}")]
    InvalidCustomId(String),
    /// Two requests of one batch share a `custom_id`.
    #[error("duplicate custom_id {0:?}")]
    DuplicateCustomId(String),
    /// A provider configuration that needs a `base_url` has none.
    #[error("{provider} provider needs base_url")]
    MissingBaseUrl {
        /// The configured provider name.
        provider: &'static str,
    },
    /// A provider configuration that needs a `model` has none.
    #[error("{provider} provider needs model")]
    MissingModel {
        /// The configured provider name.
        provider: &'static str,
    },
}

/// Errors from the provider layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A provider that needs a credential was created without one.
    #[error("missing credentials: {message}")]
    MissingCredentials {
        /// What was missing.
        message: String,
    },
    /// The HTTP client could not be built, or a request failed in transit.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    /// A response body or event was not the JSON the wire format promises.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The server answered 2xx but sent no bytes.
    #[error("empty response body")]
    EmptyResponse,
    /// A line, an event or a tool input exceeded its limit.
    #[error("{what} exceeds {max} bytes")]
    TooLarge {
        /// The buffer that overflowed.
        what: Oversize,
        /// Its limit in bytes.
        max: usize,
    },
    /// The event stream ended before its `MessageStop`.
    #[error("stream ended before message_stop")]
    UnexpectedEnd,
    /// A completed line of an event stream was not valid UTF-8.
    #[error("invalid UTF-8 in event stream: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    /// The streamed input of a tool call was not valid JSON.
    #[error("tool_use input of {name} ({id}) is not valid JSON: {source}")]
    ToolInput {
        /// The tool's name.
        name: String,
        /// The tool call's id.
        id: String,
        /// The parser's error.
        source: serde_json::Error,
    },
    /// The provider answered with an error.
    #[error("API error ({:?}, status {:?}): {}", .0.kind, .0.status, .0.message)]
    Api(ApiError),
    /// An authentication step failed.
    #[error("auth error: {0}")]
    Auth(String),
    /// A local I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// The provider factory received a name it does not know.
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    /// The request was rejected locally, before any network I/O.
    #[error("invalid request: {0}")]
    InvalidRequest(#[from] RequestError),
    /// Results were asked for a batch that has not ended.
    #[error("batch {0} has not ended")]
    BatchNotEnded(BatchId),
    /// No bytes arrived for `after`, before the headers or between chunks.
    #[error("stream idle for {after:?}")]
    IdleTimeout {
        /// The longest silence allowed.
        after: Duration,
    },
    /// Retries ran out; `last` is the final attempt's error.
    #[error("gave up after {attempts} attempts: {last}")]
    RetriesExhausted {
        /// Attempts made, the first included.
        attempts: u32,
        /// The final attempt's error.
        last: Box<Error>,
    },
}

impl Error {
    /// The class of this error; [`Error::RetriesExhausted`] reports its last
    /// attempt's class.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Api(e) => e.kind,
            Error::Http(e) => http_kind(e),
            Error::IdleTimeout { .. } => ErrorKind::Timeout,
            Error::Auth(_) | Error::MissingCredentials { .. } => ErrorKind::Auth,
            Error::InvalidRequest(_) | Error::BatchNotEnded(_) => ErrorKind::InvalidRequest,
            Error::RetriesExhausted { last, .. } => last.kind(),
            _ => ErrorKind::Other,
        }
    }

    /// True when another attempt may succeed: rate limits, overload, server
    /// errors, timeouts and network failures.
    #[must_use]
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

/// The class of a transport failure: a failed connect is network trouble
/// even when it timed out; other timeouts are timeouts; request and body
/// failures are network trouble.
fn http_kind(error: &reqwest::Error) -> ErrorKind {
    if error.is_connect() {
        ErrorKind::Network
    } else if error.is_timeout() {
        ErrorKind::Timeout
    } else if error.is_request() || error.is_body() {
        ErrorKind::Network
    } else {
        ErrorKind::Other
    }
}

#[cfg(test)]
mod tests;
