//! Message batches, provider-neutral: [`BatchProvider`] submits many
//! [`CompletionRequest`]s as one batch and collects a [`BatchResult`] per
//! request. The provider never polls; the caller's loop does.

use std::fmt;

use async_trait::async_trait;

use crate::provider::api_error::ApiError;
use crate::provider::error::{Error, RequestError};
use crate::provider::traits::{Completion, CompletionRequest};

/// A provider that runs requests as an asynchronous batch.
///
/// ```no_run
/// use llm_provider::provider::{Anthropic, BatchProvider, BatchRequest, BatchState};
/// use llm_provider::Message;
///
/// # async fn run(claude: Anthropic, request: llm_provider::CompletionRequest) -> Result<(), llm_provider::Error> {
/// let id = claude.submit(vec![BatchRequest::new("item_1", request)?]).await?;
/// if claude.status(&id).await?.state() == BatchState::Ended {
///     for result in claude.results(&id).await? {
///         println!("{}: {:?}", result.custom_id(), result.outcome());
///     }
/// }
/// # Ok(()) }
/// ```
#[async_trait]
pub trait BatchProvider: Send + Sync {
    /// Submits `requests` as one batch and returns its server-assigned id.
    /// An empty batch or a repeated `custom_id` is
    /// [`Error::InvalidRequest`], before any network I/O.
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error>;

    /// The batch's processing state and per-outcome request counts.
    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error>;

    /// The outcome of every request of an ended batch, in file order; a batch
    /// still running is [`Error::BatchNotEnded`].
    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error>;

    /// Asks the server to cancel the batch and returns its new status. Never
    /// retried, so a cancel is sent at most once.
    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error>;
}

/// One request of a batch, keyed by a caller-chosen `custom_id`.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    custom_id: String,
    request: CompletionRequest,
}

impl BatchRequest {
    /// A batch entry for `request`. `custom_id` must match
    /// `^[a-zA-Z0-9_-]{1,64}$`.
    ///
    /// # Errors
    /// [`RequestError::InvalidCustomId`] for any other `custom_id`.
    pub fn new(
        custom_id: impl Into<String>,
        request: CompletionRequest,
    ) -> Result<Self, RequestError> {
        let custom_id = custom_id.into();
        if !valid_custom_id(&custom_id) {
            return Err(RequestError::InvalidCustomId(custom_id));
        }

        Ok(Self { custom_id, request })
    }

    /// The key that ties this request to its [`BatchResult`].
    #[must_use]
    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    /// The request itself.
    #[must_use]
    pub fn request(&self) -> &CompletionRequest {
        &self.request
    }
}

/// True when `id` matches `^[a-zA-Z0-9_-]{1,64}$`.
fn valid_custom_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A batch's server-assigned identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchId(String);

impl BatchId {
    /// The identifier as the server wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for BatchId {
    fn from(id: String) -> Self {
        Self(id)
    }
}

impl fmt::Display for BatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A batch's processing state and how many of its requests ended which way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchStatus {
    state: BatchState,
    counts: BatchCounts,
}

impl BatchStatus {
    /// A status in `state` with `counts`.
    pub(crate) fn new(state: BatchState, counts: BatchCounts) -> Self {
        Self { state, counts }
    }

    /// Where the batch is in its processing.
    #[must_use]
    pub fn state(&self) -> BatchState {
        self.state
    }

    /// How many requests are processing, succeeded, errored, were canceled
    /// or expired.
    #[must_use]
    pub fn counts(&self) -> BatchCounts {
        self.counts
    }
}

/// The number of a batch's requests in each outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatchCounts {
    processing: u32,
    succeeded: u32,
    errored: u32,
    canceled: u32,
    expired: u32,
}

impl BatchCounts {
    /// The counts in the order processing, succeeded, errored, canceled,
    /// expired.
    pub(crate) fn new(
        processing: u32,
        succeeded: u32,
        errored: u32,
        canceled: u32,
        expired: u32,
    ) -> Self {
        Self {
            processing,
            succeeded,
            errored,
            canceled,
            expired,
        }
    }

    /// Requests not yet processed.
    #[must_use]
    pub fn processing(&self) -> u32 {
        self.processing
    }

    /// Requests that produced a completion.
    #[must_use]
    pub fn succeeded(&self) -> u32 {
        self.succeeded
    }

    /// Requests the provider rejected.
    #[must_use]
    pub fn errored(&self) -> u32 {
        self.errored
    }

    /// Requests canceled before processing.
    #[must_use]
    pub fn canceled(&self) -> u32 {
        self.canceled
    }

    /// Requests that expired before processing.
    #[must_use]
    pub fn expired(&self) -> u32 {
        self.expired
    }
}

/// Where a batch is in its processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchState {
    /// Requests are still being processed.
    InProgress,
    /// A cancel was asked for and is under way.
    Canceling,
    /// Every request has an outcome; results can be read.
    Ended,
}

/// The outcome of one batch request, keyed by its `custom_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchResult {
    custom_id: String,
    outcome: BatchOutcome,
}

impl BatchResult {
    /// The result for the request keyed `custom_id`.
    pub(crate) fn new(custom_id: String, outcome: BatchOutcome) -> Self {
        Self { custom_id, outcome }
    }

    /// The `custom_id` of the [`BatchRequest`] this result answers.
    #[must_use]
    pub fn custom_id(&self) -> &str {
        &self.custom_id
    }

    /// What happened to the request.
    #[must_use]
    pub fn outcome(&self) -> &BatchOutcome {
        &self.outcome
    }
}

/// What happened to one batch request.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BatchOutcome {
    /// The request produced a completion.
    Succeeded(Completion),
    /// The provider rejected the request.
    Errored(ApiError),
    /// The result line could not be read; the text is the parser's message.
    Malformed(String),
    /// The batch was canceled before this request ran.
    Canceled,
    /// The batch expired before this request ran.
    Expired,
}
