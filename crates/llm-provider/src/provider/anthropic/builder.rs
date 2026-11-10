//! [`AnthropicBuilder`]: settings for an [`Anthropic`] endpoint, collected
//! before it is built.

use std::sync::Arc;

use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{Channel, ChannelParts, Route};
use crate::provider::endpoint_builder::{EndpointBuilder, FromChannel};

/// Settings for an [`Anthropic`] endpoint. Unset values take the defaults:
/// origin `https://api.anthropic.com`,
/// [`ProviderOptions::default`](crate::ProviderOptions::default) (connect
/// 30 s, idle 90 s) and [`RetryPolicy::default`](crate::RetryPolicy::default)
/// (4 attempts).
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
pub type AnthropicBuilder = EndpointBuilder<Anthropic>;

impl EndpointBuilder<Anthropic> {
    /// A builder with the defaults, authenticated by `credential`.
    pub(crate) fn new(credential: AuthMethod) -> Self {
        let mut parts = ChannelParts::new(DEFAULT_BASE_URL);
        parts.credential = Some(credential);

        Self::from_parts(parts)
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.parts_mut().set_origin(&base_url.into());
        self
    }
}

impl FromChannel for Anthropic {
    fn from_channel(channel: Channel) -> Self {
        Self::routed(Route::root(Arc::new(channel)))
    }
}

#[cfg(test)]
mod tests;
