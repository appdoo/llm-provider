//! [`RetryingProvider`]: a provider wrapped in a [`RetryPolicy`].

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_stream::StreamExt;

use super::run::{run, Sleeper};
use super::window::{Attempt, Window};
use super::RetryPolicy;
#[cfg(feature = "batch")]
use crate::provider::batch::{BatchId, BatchProvider, BatchRequest, BatchResult, BatchStatus};
use crate::provider::error::Error;
#[cfg(feature = "batch")]
use crate::provider::error::ErrorKind;
use crate::provider::EmbedProtocol;
use crate::provider::EventStream;
use crate::provider::{ChatProtocol, CompletionRequest};

/// A provider wrapped in a [`RetryPolicy`]: every call it takes goes to the
/// inner provider until it succeeds or the policy gives up.
#[derive(Clone)]
pub(crate) struct RetryingProvider<P> {
    inner: P,
    policy: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
}

impl<P> RetryingProvider<P> {
    /// Wraps `inner`, waiting through `sleeper` between attempts.
    pub(crate) fn new(inner: P, policy: RetryPolicy, sleeper: Arc<dyn Sleeper>) -> Self {
        Self {
            inner,
            policy,
            sleeper,
        }
    }

    /// The provider every attempt goes to.
    pub(crate) fn inner(&self) -> &P {
        &self.inner
    }
}

#[cfg(feature = "batch")]
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

impl<P> fmt::Debug for RetryingProvider<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryingProvider")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl<P: ChatProtocol + Clone + 'static> ChatProtocol for RetryingProvider<P> {
    /// Retries while only `Usage` has arrived (the retry window); after the
    /// first content event every item is forwarded and never retried.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        let this = self.clone();
        let model = model.to_owned();

        Box::pin(async_stream::stream! {
            let mut backoff = this.policy.backoff();
            let mut number = 1;
            let mut events = loop {
                let attempt = Attempt { provider: &this.inner, policy: &this.policy, number };
                match attempt.open(&model, &request, &mut backoff).await {
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

    fn name(&self) -> &str {
        self.inner.name()
    }
}

#[cfg(feature = "batch")]
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

#[async_trait]
impl<P: EmbedProtocol + 'static> EmbedProtocol for RetryingProvider<P> {
    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.inner.embed(model, texts)
        })
        .await
    }
}
