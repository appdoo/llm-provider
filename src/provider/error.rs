//! The provider layer's errors: [`Error`] for every failure,
//! [`RequestError`] for a request rejected before any network I/O and
//! [`ConfigError`] for a provider configuration the factory cannot build.

use std::fmt;
use std::time::Duration;

use crate::provider::api_error::ApiError;
use crate::provider::batch::BatchId;
use crate::provider::BreakpointPosition;

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
}

/// A provider configuration the factory cannot build from.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The provider needs a credential and none was given.
    #[error("{provider} provider needs a credential")]
    MissingCredential {
        /// The configured provider's name.
        provider: &'static str,
    },
    /// The provider needs a `base_url` and the configuration has none.
    #[error("{provider} provider needs base_url")]
    MissingBaseUrl {
        /// The configured provider's name.
        provider: &'static str,
    },
    /// The provider needs a `model` and the configuration has none.
    #[error("{provider} provider needs model")]
    MissingModel {
        /// The configured provider's name.
        provider: &'static str,
    },
}

impl ConfigError {
    /// The class of this error: a missing credential is
    /// [`ErrorKind::Auth`], a missing setting [`ErrorKind::InvalidRequest`].
    fn kind(&self) -> ErrorKind {
        match self {
            ConfigError::MissingCredential { .. } => ErrorKind::Auth,
            ConfigError::MissingBaseUrl { .. } | ConfigError::MissingModel { .. } => {
                ErrorKind::InvalidRequest
            }
        }
    }
}

/// Errors from the provider layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The provider configuration was incomplete.
    #[error("invalid configuration: {0}")]
    Config(#[from] ConfigError),
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
    #[error("API error ({:?}, status {:?}): {}", .0.kind(), .0.status(), .0.message())]
    Api(ApiError),
    /// A local I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
            Error::Api(e) => e.kind(),
            Error::Http(e) => http_kind(e),
            Error::IdleTimeout { .. } => ErrorKind::Timeout,
            Error::Config(e) => e.kind(),
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
