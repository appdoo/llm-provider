//! Settings for an [`Anthropic`] client, collected before it is built.

use std::fmt;
#[cfg(test)]
use std::sync::Arc;

use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::retry::RetryPolicy;
#[cfg(test)]
use crate::provider::retry::Sleeper;
use crate::provider::traits::ProviderOptions;

/// Settings for an [`Anthropic`] client. Unset values take the defaults:
/// origin [`DEFAULT_BASE_URL`], [`ProviderOptions::default`] (connect 30 s,
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
#[must_use]
pub struct AnthropicBuilder {
    credential: AuthMethod,
    model: String,
    base_url: String,
    options: ProviderOptions,
    retry: RetryPolicy,
    #[cfg(test)]
    sleeper: Option<Arc<dyn Sleeper>>,
}

impl AnthropicBuilder {
    /// A builder with the defaults for `model`, authenticated by `credential`.
    pub(crate) fn new(credential: AuthMethod, model: String) -> Self {
        todo!()
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        todo!()
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        todo!()
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        todo!()
    }

    /// Test seam: waits through `sleeper` instead of `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        todo!()
    }

    /// The client. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> Anthropic {
        todo!()
    }
}

/// Hand-written to show the model and origin only.
impl fmt::Debug for AnthropicBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}
