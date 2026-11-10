//! [`Retrying`]: a protocol's one-attempt calls under a [`RetryPolicy`].

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

/// A protocol's one-attempt calls under a [`RetryPolicy`]: each call is
/// repeated until it succeeds or the policy gives up.
#[derive(Clone)]
pub(crate) struct Retrying<A> {
    api: A,
    policy: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
}

impl<A> Retrying<A> {
    /// Repeats the calls of `api`, waiting through `sleeper` between attempts.
    pub(crate) fn new(api: A, policy: RetryPolicy, sleeper: Arc<dyn Sleeper>) -> Self {
        Self {
            api,
            policy,
            sleeper,
        }
    }

    /// The API every attempt goes to.
    pub(crate) fn api(&self) -> &A {
        &self.api
    }
}

/// True for a batch submit the API rejected with a transient status, so no
/// batch exists yet and sending again cannot create a second one.
#[cfg(feature = "batch")]
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

impl<A> fmt::Debug for Retrying<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Retrying")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl<A: ChatProtocol + Clone + 'static> ChatProtocol for Retrying<A> {
    /// Retries while only `Usage` has arrived (the retry window); after the
    /// first content event every item is forwarded and never retried.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        let this = self.clone();
        let model = model.to_owned();

        Box::pin(async_stream::stream! {
            let mut backoff = this.policy.backoff();
            let mut number = 1;
            let mut events = loop {
                let attempt = Attempt { api: &this.api, policy: &this.policy, number };
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
        self.api.name()
    }
}

#[cfg(feature = "batch")]
#[async_trait]
impl<A: BatchProvider + 'static> BatchProvider for Retrying<A> {
    /// Retries only an API rejection with a transient status: after a
    /// timeout or a broken connection a batch may already exist.
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, submit_retry_if, || {
            self.api.submit(requests.clone())
        })
        .await
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.api.status(id)
        })
        .await
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.api.results(id)
        })
        .await
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.api.cancel(id).await
    }
}

#[async_trait]
impl<A: EmbedProtocol + 'static> EmbedProtocol for Retrying<A> {
    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        let sleeper = Arc::clone(&self.sleeper);
        run(&self.policy, sleeper, Error::is_retryable, || {
            self.api.embed(model, texts)
        })
        .await
    }
}
