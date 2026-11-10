//! Settings for an [`Anthropic`] endpoint, collected before it is built.

use std::sync::Arc;

use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{Channel, ChannelParts};
use crate::provider::retry::RetryPolicy;
#[cfg(test)]
use crate::provider::retry::Sleeper;
use crate::provider::ProviderOptions;

/// Settings for an [`Anthropic`] endpoint. Unset values take the defaults:
/// origin `https://api.anthropic.com`, [`ProviderOptions::default`] (connect
/// 30 s, idle 90 s) and [`RetryPolicy::default`] (4 attempts).
///
/// ```no_run
/// use llm_provider::{Anthropic, AuthMethod, RetryPolicy};
///
/// let claude = Anthropic::builder(AuthMethod::Bearer("sk-ant-oat01-…".into()))
///     .base_url("http://localhost:8080")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = claude;
/// ```
///
/// No setter turns it into a Bedrock endpoint; that is [`Bedrock`](crate::Bedrock):
///
/// ```compile_fail
/// use llm_provider::Anthropic;
///
/// let k = "k";
/// let claude = Anthropic::builder(k).bedrock("eu-central-1");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct AnthropicBuilder {
    parts: ChannelParts,
}

impl AnthropicBuilder {
    /// A builder with the defaults, authenticated by `credential`.
    pub(crate) fn new(credential: AuthMethod) -> Self {
        let mut parts = ChannelParts::new(DEFAULT_BASE_URL.to_string());
        parts.credential = Some(credential);

        Self { parts }
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.parts.origin = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.parts.options = options;
        self
    }

    /// Sends through `client`, one the caller configured with a proxy, root
    /// certificates or a shared pool, instead of a client built from
    /// [`timeouts`](Self::timeouts). Its timeouts and redirect policy are the
    /// caller's; the client this crate builds follows no redirect, so the
    /// credential never reaches a second origin.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.parts.client = Some(client);
        self
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.parts.retry = policy;
        self
    }

    /// Waits between attempts through `sleeper` instead of
    /// `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.parts.sleeper = Some(sleeper);
        self
    }

    /// The endpoint. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> Anthropic {
        Anthropic::on(&Arc::new(Channel::new(self.parts)), "")
    }
}

#[cfg(test)]
mod tests;
