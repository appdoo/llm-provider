//! [`BedrockBuilder`]: settings for [`Bedrock`], collected before it is
//! built.

use super::Bedrock;
use crate::config::ConfigError;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{fixed_origin, parse_origin, ChannelKind, ChannelParts};
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
/// let bedrock = Bedrock::builder("ABSK…", "eu-central-1")?
///     .timeouts(ProviderOptions::default().connect_timeout(Duration::from_secs(5)))
///     .build();
/// # let _ = bedrock;
/// # Ok::<(), llm_provider::config::ConfigError>(())
/// ```
pub type BedrockBuilder = EndpointBuilder<Bedrock>;

impl EndpointBuilder<Bedrock> {
    /// A builder with the defaults for `region`, authenticated by
    /// `credential`; a region not of the form `eu-central-1` is
    /// [`ConfigError::InvalidRegion`].
    pub(crate) fn new(credential: AuthMethod, region: String) -> Result<Self, ConfigError> {
        if !is_region(&region) {
            return Err(ConfigError::InvalidRegion(region));
        }

        let origin = format!("https://bedrock-runtime.{region}.amazonaws.com");
        let mut parts = ChannelParts::new(fixed_origin(&origin));
        parts.kind = ChannelKind::Bedrock;
        parts.credential = Some(credential);

        Ok(Self::from_parts(parts))
    }

    /// Sends requests to another origin, such as a proxy or a test server;
    /// the routes `/anthropic` and `/openai/v1` stay. A trailing `/` is
    /// ignored.
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

/// True when `region` matches `[a-z]{2}(-[a-z]+)+-\d`, as `eu-central-1`
/// or `us-gov-west-1` do.
fn is_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    let [first, middle @ .., last] = parts.as_slice() else {
        return false;
    };
    let letters = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase());

    first.len() == 2
        && letters(first)
        && !middle.is_empty()
        && middle.iter().all(|part| letters(part))
        && last.len() == 1
        && last.bytes().all(|b| b.is_ascii_digit())
}
