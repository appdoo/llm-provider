//! [`BedrockBuilder`]: settings for [`Bedrock`], collected before it is
//! built.

use super::Bedrock;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{ChannelKind, ChannelParts};
use crate::provider::endpoint_builder::EndpointBuilder;

/// Settings for [`Bedrock`]: the region, credential, timeouts and retry
/// policy its endpoints share. Unset values take the defaults:
/// origin `https://bedrock-runtime.{region}.amazonaws.com`,
/// [`ProviderOptions::default`](crate::ProviderOptions::default) (connect
/// 30 s, idle 90 s) and [`RetryPolicy::default`](crate::RetryPolicy::default)
/// (4 attempts).
///
/// ```no_run
/// use std::time::Duration;
/// use llm_provider::{Bedrock, ProviderOptions};
///
/// let bedrock = Bedrock::builder("ABSK…", "eu-central-1")
///     .timeouts(ProviderOptions::default().connect_timeout(Duration::from_secs(5)))
///     .build();
/// # let _ = bedrock;
/// ```
pub type BedrockBuilder = EndpointBuilder<Bedrock>;

impl EndpointBuilder<Bedrock> {
    /// A builder with the defaults for `region`, authenticated by
    /// `credential`.
    pub(crate) fn new(credential: AuthMethod, region: String) -> Self {
        let mut parts =
            ChannelParts::new(&format!("https://bedrock-runtime.{region}.amazonaws.com"));
        parts.kind = ChannelKind::Bedrock { region };
        parts.credential = Some(credential);

        Self::from_parts(parts)
    }

    /// Sends requests to another origin, such as a proxy or a test server;
    /// the routes `/anthropic` and `/openai/v1` stay. A trailing `/` is
    /// ignored.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.parts_mut().set_origin(&base_url.into());
        self
    }
}
