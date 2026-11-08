//! Settings for an [`Anthropic`] client, collected before it is built.

use crate::provider::anthropic::api::AnthropicApi;
use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::provider::ProviderOptions;
use crate::transport::Http;

/// Settings for an [`Anthropic`] client. Unset values take the defaults:
/// origin `https://api.anthropic.com`, [`ProviderOptions::default`] (connect 30 s,
/// idle 90 s) and [`RetryPolicy::default`] (4 attempts).
///
/// ```no_run
/// use llm_provider::{Anthropic, AuthMethod, RetryPolicy};
///
/// let claude = Anthropic::builder(AuthMethod::Bearer("sk-ant-oat01-…".into()), "claude-sonnet-5-5")
///     .base_url("http://localhost:8080")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = claude;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct AnthropicBuilder {
    pub(super) credential: AuthMethod,
    pub(super) model: String,
    pub(super) base_url: String,
    pub(super) route: &'static str,
    pub(super) region: Option<String>,
    options: ProviderOptions,
    retry: RetryPolicy,
    http_client: Option<reqwest::Client>,
}

impl AnthropicBuilder {
    /// A builder with the defaults for `model`, authenticated by `credential`.
    pub(crate) fn new(credential: AuthMethod, model: String) -> Self {
        Self {
            credential,
            model,
            base_url: DEFAULT_BASE_URL.to_string(),
            route: "",
            region: None,
            options: ProviderOptions::default(),
            retry: RetryPolicy::default(),
            http_client: None,
        }
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Sends requests to Claude on Amazon Bedrock in `region`: origin
    /// `https://bedrock-runtime.{region}.amazonaws.com`, route `/anthropic`,
    /// the credential a Bedrock API key sent as `x-api-key`. A later
    /// [`base_url`](Self::base_url) replaces the origin and keeps the route.
    /// A region not of the form `eu-central-1` fails the first request with
    /// [`Error::InvalidRegion`](crate::Error::InvalidRegion), before any I/O.
    ///
    /// ```no_run
    /// use llm_provider::Anthropic;
    ///
    /// let claude = Anthropic::builder("ABSK…", "eu.anthropic.claude-haiku-5-5")
    ///     .bedrock("eu-central-1")
    ///     .build();
    /// # let _ = claude;
    /// ```
    pub fn bedrock(self, region: impl Into<String>) -> Self {
        let _ = region;
        todo!()
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.options = options;
        self
    }

    /// Sends through `client`, one the caller configured with a proxy, root
    /// certificates or a shared pool, instead of a client built from
    /// [`timeouts`](Self::timeouts). Its timeouts and redirect policy are the
    /// caller's; the client this crate builds follows no redirect, so the
    /// credential never reaches a second origin.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.retry = policy;
        self
    }

    /// The client. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> Anthropic {
        let retry = self.retry;
        Anthropic::from_retrying(RetryingProvider::new(self.api(), retry))
    }

    /// One attempt against the API with these settings, without retry.
    pub(crate) fn api(mut self) -> AnthropicApi {
        let options = self.options;
        let http = self
            .http_client
            .take()
            .map_or_else(|| Http::new(options), Http::from);

        AnthropicApi::new(self, http)
    }
}

#[cfg(test)]
mod tests;
