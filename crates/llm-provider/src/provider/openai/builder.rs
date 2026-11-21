//! [`OpenAiBuilder`]: settings for an [`OpenAi`] endpoint, collected before
//! it is built.

use crate::provider::auth::AuthMethod;
use crate::provider::channel::ChannelParts;
use crate::provider::endpoint_builder::EndpointBuilder;
use crate::provider::openai::client::OpenAi;

/// Settings for an [`OpenAi`] endpoint. Unset values take the defaults: no
/// credential (no `Authorization` header, as a local server wants),
/// [`ProviderOptions::default`](crate::ProviderOptions::default) (connect
/// 30 s, idle 90 s) and [`RetryPolicy::default`](crate::RetryPolicy::default)
/// (4 attempts).
///
/// ```no_run
/// use llm_provider::{OpenAi, RetryPolicy};
///
/// let openai = OpenAi::builder("https://api.openai.com/v1")?
///     .credential("sk-…")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = openai;
/// # Ok::<(), llm_provider::config::ConfigError>(())
/// ```
pub type OpenAiBuilder = EndpointBuilder<OpenAi>;

impl EndpointBuilder<OpenAi> {
    /// A builder with the defaults for the server at `origin`; a trailing
    /// `/` is ignored.
    pub(crate) fn new(origin: reqwest::Url) -> Self {
        Self::from_parts(ChannelParts::new(origin))
    }

    /// Sends `credential` as `Authorization: Bearer <key>`. A plain string is
    /// an API key.
    pub fn credential(mut self, credential: impl Into<AuthMethod>) -> Self {
        self.parts_mut().credential = Some(credential.into());
        self
    }
}
