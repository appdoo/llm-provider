//! Retry with exponential backoff and `Retry-After` for transient provider
//! failures. All re-sending in the crate goes through this module:
//! [`RetryingProvider`] wraps a provider's transport, and [`run`] retries one
//! call.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::provider::batch::{BatchId, BatchProvider, BatchRequest, BatchResult, BatchStatus};
use crate::provider::error::{Error, ErrorKind};
use crate::provider::traits::{CompletionRequest, LlmProvider, ProviderInfo};
use crate::provider::{EventStream, StreamEvent};

/// How often to try a request and how long to wait between attempts.
/// The default is 4 attempts, a base wait of 1 s and a cap of 60 s.
///
/// ```no_run
/// use std::time::Duration;
/// use llm_provider::RetryPolicy;
///
/// let patient = RetryPolicy::default().max_attempts(6).cap(Duration::from_secs(120));
/// let once = RetryPolicy::none();
/// # let _ = (patient, once);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u32,
    base: Duration,
    cap: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// One attempt, no retry.
    #[must_use]
    pub fn none() -> Self {
        Self::default().max_attempts(1)
    }

    /// Sets the total number of attempts, the first included; 0 counts as 1.
    #[must_use]
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n.max(1);
        self
    }

    /// Sets the wait before the second attempt, before jitter; each later
    /// wait doubles it.
    #[must_use]
    pub fn base(mut self, wait: Duration) -> Self {
        self.base = wait;
        self
    }

    /// Sets the longest wait; a larger `Retry-After` gives up instead.
    #[must_use]
    pub fn cap(mut self, wait: Duration) -> Self {
        self.cap = wait;
        self
    }
}

/// Waits between attempts. Tests substitute a recorder.
#[async_trait]
pub(crate) trait Sleeper: Send + Sync {
    /// Waits for `duration`.
    async fn sleep(&self, duration: Duration);
}

/// The [`Sleeper`] on `tokio::time::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// What to do after an attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Not retryable: return the error unchanged.
    Fail,
    /// Retryable, but no attempt or wait is left.
    Exhausted,
    /// Sleep this long, then try again.
    Wait(Duration),
}

/// The decision after attempt `n` (counted from 1) failed with `error`:
/// `Retry-After` when the answer carries one, else backoff with jitter.
pub(crate) fn decide(
    policy: &RetryPolicy,
    n: u32,
    error: &Error,
    retry_if: fn(&Error) -> bool,
) -> Decision {
    todo!()
}

/// Runs `attempt` until it succeeds or [`decide`] says stop; running out is
/// [`Error::RetriesExhausted`].
pub(crate) async fn run<T, F, Fut>(
    policy: &RetryPolicy,
    sleeper: &dyn Sleeper,
    retry_if: fn(&Error) -> bool,
    attempt: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    todo!()
}

/// True for a batch submit the API rejected with a transient status, so no
/// batch exists yet and sending again cannot create a second one.
fn submit_retry_if(error: &Error) -> bool {
    matches!(
        error,
        Error::Api(api) if api.status.is_some()
            && matches!(api.kind, ErrorKind::RateLimited | ErrorKind::Overloaded | ErrorKind::Server)
    )
}

/// How one streaming attempt opened.
enum Window {
    /// The first content event arrived: forward the buffered usage, then
    /// the rest of the stream, never retried.
    Open(Vec<StreamEvent>, EventStream),
    /// A retryable error inside the window: wait, then try again.
    Retry(Duration),
    /// The attempt ended for good: yield these items and stop.
    Fail(Vec<Result<StreamEvent, Error>>),
}

/// A provider transport wrapped in a [`RetryPolicy`]. Clones share the
/// transport.
pub(crate) struct RetryingProvider<P> {
    inner: Arc<P>,
    policy: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
}

impl<P> RetryingProvider<P> {
    /// Wraps `inner`, waiting with [`TokioSleeper`] between attempts.
    pub(crate) fn new(inner: P, policy: RetryPolicy) -> Self {
        Self {
            inner: Arc::new(inner),
            policy,
            sleeper: Arc::new(TokioSleeper),
        }
    }

    /// Test seam: waits through `sleeper` instead of `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// The wrapped transport.
    pub(crate) fn transport(&self) -> &P {
        &self.inner
    }

    /// The retry policy in force.
    pub(crate) fn policy(&self) -> RetryPolicy {
        self.policy
    }
}

impl<P: LlmProvider + 'static> RetryingProvider<P> {
    /// Reads attempt `n` up to its first content event and decides: open,
    /// retry or fail. Only `Usage` events may precede the decision.
    async fn open_window(&self, request: &CompletionRequest, n: u32) -> Window {
        todo!()
    }
}

impl<P> Clone for RetryingProvider<P> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            policy: self.policy,
            sleeper: Arc::clone(&self.sleeper),
        }
    }
}

impl<P> fmt::Debug for RetryingProvider<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryingProvider")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<P: LlmProvider + 'static> LlmProvider for RetryingProvider<P> {
    /// Retries while only `Usage` has arrived (the retry window); after the
    /// first content event every item is forwarded and never retried.
    fn stream(&self, request: CompletionRequest) -> EventStream {
        todo!()
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
    }
}

#[async_trait]
impl<P: BatchProvider + 'static> BatchProvider for RetryingProvider<P> {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        todo!()
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        todo!()
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        todo!()
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.cancel(id).await
    }
}

#[cfg(test)]
mod tests;
