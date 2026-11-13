//! [`OpenAiBuilder`]: settings for an [`OpenAi`] endpoint, collected before
//! it is built.

use std::sync::Arc;

use crate::provider::auth::AuthMethod;
use crate::provider::channel::{Channel, ChannelParts, Route};
use crate::provider::endpoint_builder::{EndpointBuilder, FromChannel};
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
/// let openai = OpenAi::builder("https://api.openai.com/v1")
///     .credential("sk-…")
///     .retry(RetryPolicy::none())
///     .build();
/// # let _ = openai;
/// ```
pub type OpenAiBuilder = EndpointBuilder<OpenAi>;

impl EndpointBuilder<OpenAi> {
    /// A builder with the defaults for the server at `base_url`; a trailing
    /// `/` is ignored.
    pub(crate) fn new(base_url: &str) -> Self {
        Self::from_parts(ChannelParts::new(base_url))
    }

    /// Sends `credential` as `Authorization: Bearer <key>`. A plain string is
    /// an API key.
    pub fn credential(mut self, credential: impl Into<AuthMethod>) -> Self {
        self.parts_mut().credential = Some(credential.into());
        self
    }
}

impl FromChannel for OpenAi {
    fn from_channel(channel: Channel) -> Self {
        Self::routed(Route::root(Arc::new(channel)))
    }
}
