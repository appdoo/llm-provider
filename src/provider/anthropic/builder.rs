//! Settings for an [`Anthropic`] client, collected before it is built.

use crate::provider::anthropic::api::AnthropicApi;
use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::provider::traits::ProviderOptions;
use crate::transport::Http;

/// Settings for an [`Anthropic`] client. Unset values take the defaults:
/// origin `https://api.anthropic.com`, [`ProviderOptions::default`] (connect 30 s,
/// idle 90 s) and [`RetryPolicy::default`] (4 attempts).
///
/// ```no_run
/// use llm_provider::{Anthropic, AuthMethod, RetryPolicy};
///
/// let claude = Anthropic::builder(AuthMethod::Bearer("sk-ant-oat01-…".into()), "claude-sonnet-4-6")
///     .base_url("http://localhost:8080")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = claude;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct AnthropicBuilder {
    credential: AuthMethod,
    model: String,
    base_url: String,
    options: ProviderOptions,
    retry: RetryPolicy,
}

impl AnthropicBuilder {
    /// A builder with the defaults for `model`, authenticated by `credential`.
    pub(crate) fn new(credential: AuthMethod, model: String) -> Self {
        Self {
            credential,
            model,
            base_url: DEFAULT_BASE_URL.to_string(),
            options: ProviderOptions::default(),
            retry: RetryPolicy::default(),
        }
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.options = options;
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
        let http = Http::new(self.options);
        let api = AnthropicApi::new(http, self.credential, self.model, &self.base_url);
        Anthropic::from_transport(RetryingProvider::new(api, self.retry))
    }
}
