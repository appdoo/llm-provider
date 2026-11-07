//! The Anthropic client: [`Anthropic`], the public front with retry, and
//! [`AnthropicApi`], the transport that sends one attempt.

use std::fmt;

use async_trait::async_trait;

use crate::provider::anthropic::builder::AnthropicBuilder;
use crate::provider::ask::Request;
use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::retry::RetryingProvider;
use crate::provider::traits::{CompletionRequest, LlmProvider, ProviderInfo};
use crate::provider::EventStream;
use crate::transport::Http;

/// The Messages API origin requests go to unless
/// [`base_url`](AnthropicBuilder::base_url) names another.
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// The model the provider factory uses when the configuration names none.
pub(crate) const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
/// The `anthropic-version` header every request carries.
const API_VERSION: &str = "2023-06-01";
/// The `anthropic-beta` header an OAuth token needs.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// The path of the Messages endpoint.
const MESSAGES_PATH: &str = "/v1/messages";

/// A client for the Anthropic Messages API that streams completions and runs
/// message batches, retrying transient failures. Clones share one HTTP
/// connection pool.
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
    /// let claude = Anthropic::builder("sk-ant-…", "claude-sonnet-4-6")
    ///     .timeouts(ProviderOptions { connect_timeout: Duration::from_secs(5), idle_gap: Duration::from_secs(30) })
    ///     .build();
    /// # let _ = claude;
    /// ```
    pub fn builder(
        credential: impl Into<AuthMethod>,
        model: impl Into<String>,
    ) -> AnthropicBuilder {
        todo!()
    }

    /// A client for `model` with the defaults. A plain string is an API key;
    /// pass an OAuth token as [`AuthMethod::Bearer`]. Use
    /// [`builder`](Self::builder) for another origin, timeouts or
    /// [retry policy](crate::RetryPolicy).
    ///
    /// ```no_run
    /// use llm_provider::{Anthropic, AuthMethod};
    ///
    /// let claude = Anthropic::new(AuthMethod::Bearer("sk-ant-oat01-…".into()), "claude-sonnet-4-6");
    /// # let _ = claude;
    /// ```
    #[must_use]
    pub fn new(credential: impl Into<AuthMethod>, model: impl Into<String>) -> Self {
        todo!()
    }

    /// Wraps a transport and its retry policy; only
    /// [`AnthropicBuilder::build`] calls it.
    pub(crate) fn from_transport(inner: RetryingProvider<AnthropicApi>) -> Self {
        todo!()
    }

    /// The retrying transport, for the batch implementation.
    pub(crate) fn inner(&self) -> &RetryingProvider<AnthropicApi> {
        todo!()
    }

    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] of the request.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        todo!()
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        todo!()
    }
}

/// Hand-written so the credential never reaches logs: shows the model and
/// origin only.
impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

#[async_trait]
impl LlmProvider for Anthropic {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        todo!()
    }

    fn info(&self) -> ProviderInfo {
        todo!()
    }
}

/// One attempt against the Messages API: the connection pool, the
/// credential, the default model and the origin. [`Anthropic`] wraps it in
/// retry; clones share the connection pool.
#[derive(Clone, Debug)]
pub(crate) struct AnthropicApi {
    http: Http,
    credential: AuthMethod,
    model: String,
    base_url: String,
}

impl AnthropicApi {
    /// A transport for `model` at `base_url`; a trailing `/` is dropped.
    pub(crate) fn new(http: Http, credential: AuthMethod, model: String, base_url: &str) -> Self {
        todo!()
    }

    /// The model a request with [`ModelSelection::Default`](crate::provider::ModelSelection::Default) uses.
    pub(crate) fn model(&self) -> &str {
        todo!()
    }

    /// The API origin without a trailing `/`.
    pub(crate) fn base_url(&self) -> &str {
        todo!()
    }

    /// The connection pool requests are sent through.
    pub(crate) fn http(&self) -> &Http {
        todo!()
    }

    /// A request to `path` on the origin carrying `anthropic-version` and the
    /// credential: `x-api-key` for an API key; `Authorization: Bearer` plus
    /// `anthropic-beta: oauth-2025-04-20` for an OAuth token. The one place
    /// Anthropic headers are set.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, Error> {
        todo!()
    }
}

#[async_trait]
impl LlmProvider for AnthropicApi {
    /// Renders the streaming body (an invalid breakpoint layout fails before
    /// any I/O), posts it to the Messages endpoint and decodes the event
    /// stream.
    fn stream(&self, request: CompletionRequest) -> EventStream {
        todo!()
    }

    fn info(&self) -> ProviderInfo {
        todo!()
    }
}
