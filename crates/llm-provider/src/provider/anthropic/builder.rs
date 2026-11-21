//! [`AnthropicBuilder`]: settings for an [`Anthropic`] endpoint, collected
//! before it is built.

use crate::config::ConfigError;
use crate::provider::anthropic::client::{Anthropic, DEFAULT_BASE_URL};
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{fixed_origin, parse_origin, ChannelParts};
use crate::provider::endpoint_builder::EndpointBuilder;

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
///     .base_url("http://localhost:8080")?
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = claude;
/// # Ok::<(), llm_provider::config::ConfigError>(())
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
        let mut parts = ChannelParts::new(fixed_origin(DEFAULT_BASE_URL));
        parts.credential = Some(credential);

        Self::from_parts(parts)
    }

    /// Sends requests to another API origin, such as a proxy or a test
    /// server. A trailing `/` is ignored.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidOrigin`] when `base_url` does not parse as an
    /// absolute URL, [`ConfigError::UnsupportedScheme`] when its scheme is
    /// neither `http` nor `https`.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Result<Self, ConfigError> {
        self.parts_mut().set_origin(parse_origin(&base_url.into())?);
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
