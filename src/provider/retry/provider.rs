//! [`RetryingProvider`]: a provider wrapped in a [`RetryPolicy`].

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_stream::StreamExt;

use super::run::{run, Sleeper, TokioSleeper};
use super::window::{Attempt, Window};
use super::RetryPolicy;
use crate::provider::batch::{BatchId, BatchProvider, BatchRequest, BatchResult, BatchStatus};
use crate::provider::error::{Error, ErrorKind};
use crate::provider::EventStream;
use crate::provider::{CompletionRequest, LlmProvider, ProviderInfo};

/// A provider wrapped in a [`RetryPolicy`]: every call it takes goes to the
/// inner provider until it succeeds or the policy gives up. Clones share the
/// inner provider.
pub(crate) struct RetryingProvider<P> {
    inner: Arc<P>,
    policy: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
}

impl<P> RetryingProvider<P> {
    /// Wraps `inner`, waiting with `tokio::time::sleep` between attempts.
    pub(crate) fn new(inner: P, policy: RetryPolicy) -> Self {
        Self {
            inner: Arc::new(inner),
            policy,
            sleeper: Arc::new(TokioSleeper),
        }
    }

    /// Waits through `sleeper` instead of `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn with_sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.sleeper = sleeper;
        self
    }

    /// The provider every attempt goes to.
    pub(crate) fn inner(&self) -> &P {
        &self.inner
    }

    /// The retry policy in force.
    pub(crate) fn policy(&self) -> RetryPolicy {
        self.policy
    }
}

/// True for a batch submit the API rejected with a transient status, so no
/// batch exists yet and sending again cannot create a second one.
fn submit_retry_if(error: &Error) -> bool {
    let Error::Api(api) = error else {
        return false;
    };

    api.status().is_some()
        && matches!(
            api.kind(),
            ErrorKind::RateLimited | ErrorKind::Overloaded | ErrorKind::Server
        )
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
        let this = self.clone();
        Box::pin(async_stream::stream! {
            let mut backoff = this.policy.backoff();
            let mut number = 1;
            let mut events = loop {
                let attempt = Attempt { provider: &*this.inner, policy: &this.policy, number };
                match attempt.open(&request, &mut backoff).await {
                    Window::Open(events) => break events,
                    Window::Fail(items) => break Box::pin(tokio_stream::iter(items)) as EventStream,
                    Window::Retry(wait) => this.sleeper.sleep(wait).await,
                }
                number += 1;
            };

            while let Some(item) = events.next().await {
                yield item;
            }
        })
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
    }
}

#[async_trait]
impl<P: BatchProvider + 'static> BatchProvider for RetryingProvider<P> {
    /// Retries only an API rejection with a transient status: after a
    /// timeout or a broken connection a batch may already exist.
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, submit_retry_if, || {
            self.inner.submit(requests.clone())
        })
        .await
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.inner.status(id)
        })
        .await
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.inner.results(id)
        })
        .await
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.cancel(id).await
    }
}
