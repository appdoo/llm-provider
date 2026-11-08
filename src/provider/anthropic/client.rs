//! [`Anthropic`]: the public client, which sends every request through its
//! retry policy to the Messages API.

use std::fmt;
#[cfg(test)]
use std::sync::Arc;

use async_trait::async_trait;

use crate::provider::anthropic::api::AnthropicApi;
use crate::provider::anthropic::builder::AnthropicBuilder;
use crate::provider::ask::{Ask, Request};
use crate::provider::auth::AuthMethod;
#[cfg(feature = "batch")]
use crate::provider::batch::{BatchId, BatchProvider, BatchRequest, BatchResult, BatchStatus};
use crate::provider::error::Error;
use crate::provider::retry::RetryingProvider;
#[cfg(test)]
use crate::provider::retry::{RetryPolicy, Sleeper};
use crate::provider::EventStream;
use crate::provider::{CompletionRequest, LlmProvider, ProviderInfo};

/// The Messages API origin requests go to unless
/// [`base_url`](AnthropicBuilder::base_url) names another.
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// A client for the Anthropic Messages API that streams completions,
/// retrying transient failures; with the `batch` feature it also runs
/// message batches. Clones share one HTTP connection pool.
#[derive(Clone)]
pub struct Anthropic {
    inner: RetryingProvider<AnthropicApi>,
}

impl Anthropic {
    /// A builder for a client with another origin, timeouts or retry policy.
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use llm_provider::{Anthropic, ProviderOptions};
    ///
    /// let claude = Anthropic::builder("sk-ant-…", "claude-sonnet-5-5")
    ///     .timeouts(ProviderOptions::default().connect_timeout(Duration::from_secs(5)))
    ///     .build();
    /// # let _ = claude;
    /// ```
    pub fn builder(
        credential: impl Into<AuthMethod>,
        model: impl Into<String>,
    ) -> AnthropicBuilder {
        AnthropicBuilder::new(credential.into(), model.into())
    }

    /// A client for `model` with the defaults. A plain string is an API key;
    /// pass an OAuth token as [`AuthMethod::Bearer`]. Use
    /// [`builder`](Self::builder) for another origin, timeouts or
    /// [retry policy](crate::RetryPolicy).
    ///
    /// ```no_run
    /// use llm_provider::{Anthropic, AuthMethod};
    ///
    /// let claude = Anthropic::new(AuthMethod::Bearer("sk-ant-oat01-…".into()), "claude-sonnet-5-5");
    /// # let _ = claude;
    /// ```
    #[must_use]
    pub fn new(credential: impl Into<AuthMethod>, model: impl Into<String>) -> Self {
        Self::builder(credential, model).build()
    }

    /// A client for Claude on Amazon Bedrock in `region`, authenticated by a
    /// Bedrock API key; the same as
    /// `Anthropic::builder(key, model).bedrock(region).build()`, see
    /// [`AnthropicBuilder::bedrock`]. `model` is a Bedrock model or inference
    /// profile id.
    ///
    /// ```no_run
    /// use llm_provider::Anthropic;
    ///
    /// let claude = Anthropic::bedrock("ABSK…", "eu-central-1", "eu.anthropic.claude-haiku-5-5");
    /// # let _ = claude;
    /// ```
    #[must_use]
    pub fn bedrock(
        key: impl Into<AuthMethod>,
        region: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::builder(key, model).bedrock(region).build()
    }

    /// Wraps one-attempt calls in their retry policy; only
    /// [`AnthropicBuilder::build`] calls it.
    pub(crate) fn from_retrying(inner: RetryingProvider<AnthropicApi>) -> Self {
        Self { inner }
    }

    /// Waits between attempts through `sleeper` instead of
    /// `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn with_sleeper(self, sleeper: Arc<dyn Sleeper>) -> Self {
        Self::from_retrying(self.inner.with_sleeper(sleeper))
    }

    /// The retry policy the client sends through.
    #[cfg(test)]
    pub(crate) fn policy(&self) -> RetryPolicy {
        self.inner.policy()
    }

    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] of the request.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        Ask::ask(self, prompt).await
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Ask::request(self, prompt)
    }
}

/// Hand-written so the credential never reaches logs: shows the model and
/// origin only.
impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api = self.inner.inner();
        f.debug_struct("Anthropic")
            .field("model", &api.model())
            .field("base_url", &api.base_url())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl LlmProvider for Anthropic {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        self.inner.stream(request)
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
    }
}

/// Message batches through the retry policy; `cancel` is never retried.
#[cfg(feature = "batch")]
#[async_trait]
impl BatchProvider for Anthropic {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        self.inner.submit(requests).await
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.status(id).await
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        self.inner.results(id).await
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.cancel(id).await
    }
}

#[cfg(test)]
mod tests;
