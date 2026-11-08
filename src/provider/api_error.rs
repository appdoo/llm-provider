//! [`ApiError`]: an error answer from a provider API, read from an HTTP
//! response or an in-stream error event.

use std::time::Duration;

use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::provider::error::ErrorKind;

/// An error answer from a provider API, as an HTTP response or an in-stream
/// error event.
///
/// ```
/// use llm_provider::provider::{ApiError, ErrorKind};
///
/// let overloaded = ApiError::new(ErrorKind::Overloaded, "try later").with_status(529);
/// assert_eq!(overloaded.status(), Some(529));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    kind: ErrorKind,
    status: Option<u16>,
    message: String,
    request_id: Option<String>,
    retry_after: Option<Duration>,
}

impl ApiError {
    /// An error answer of `kind` without status, request id or
    /// `Retry-After`, for a provider or a test double that builds one by hand.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            status: None,
            message: message.into(),
            request_id: None,
            retry_after: None,
        }
    }

    /// Sets the HTTP status the answer came with.
    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the server's `Retry-After` wait.
    #[must_use]
    pub fn with_retry_after(mut self, wait: Duration) -> Self {
        self.retry_after = Some(wait);
        self
    }

    /// The class of the failure.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The HTTP status; `None` for an in-stream error event or a batch result.
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        self.status
    }

    /// The envelope's message, or the raw body when there is no envelope.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Header `request-id` (Anthropic) or `x-request-id` (OpenAI).
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Header `retry-after`, integer seconds only.
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    /// The error carried by a non-2xx HTTP response: the kind comes from the
    /// envelope's `error.type` when it is a known one, else from `status`.
    pub(crate) fn from_response(status: u16, headers: &HeaderMap, body: &str) -> Self {
        let error = serde_json::from_str::<Envelope>(body)
            .ok()
            .and_then(|envelope| envelope.error);
        let kind = error
            .as_ref()
            .and_then(|e| e.kind.as_deref())
            .and_then(kind_from_type)
            .unwrap_or_else(|| kind_from_status(status));

        Self {
            kind,
            status: Some(status),
            message: error
                .and_then(|e| e.message)
                .unwrap_or_else(|| body.to_string()),
            request_id: request_id(headers),
            retry_after: retry_after(headers),
        }
    }

    /// The error carried by an Anthropic in-stream `error` event or an
    /// errored batch result, from its `error.type` and `error.message`.
    pub(crate) fn from_anthropic_event(error_type: &str, message: impl Into<String>) -> Self {
        let kind = kind_from_type(error_type).unwrap_or(ErrorKind::Other);
        Self::new(kind, message)
    }
}

/// Header `request-id`, else `x-request-id`.
fn request_id(headers: &HeaderMap) -> Option<String> {
    header(headers, "request-id")
        .or_else(|| header(headers, "x-request-id"))
        .map(str::to_string)
}

/// Header `retry-after` in whole seconds; an HTTP date reads as none.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let seconds = header(headers, "retry-after")?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}

/// The value of header `name`, when it is visible ASCII.
fn header<'h>(headers: &'h HeaderMap, name: &str) -> Option<&'h str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The envelope of an error body: `{"error": {"type": …, "message": …}}`,
/// as Anthropic and OpenAI-compatible servers write it.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    error: Option<EnvelopeError>,
}

/// The `error` object of an [`Envelope`] or of an OpenAI-compatible
/// in-stream error chunk.
#[derive(Debug, Deserialize)]
pub(crate) struct EnvelopeError {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl From<EnvelopeError> for ApiError {
    /// A known `type` decides the kind, else [`ErrorKind::Other`].
    fn from(error: EnvelopeError) -> Self {
        let kind = error.kind.as_deref().and_then(kind_from_type);
        Self::new(
            kind.unwrap_or(ErrorKind::Other),
            error.message.unwrap_or_default(),
        )
    }
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

#[cfg(test)]
mod tests;
