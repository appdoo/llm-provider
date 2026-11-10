//! Settings for a [`Bedrock`] endpoint, collected before it is built.

#[cfg(test)]
use std::sync::Arc;

use super::Bedrock;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{Channel, ChannelKind, ChannelParts};
use crate::provider::retry::RetryPolicy;
#[cfg(test)]
use crate::provider::retry::Sleeper;
use crate::provider::ProviderOptions;

/// Settings for a [`Bedrock`] endpoint. Unset values take the defaults:
/// origin `https://bedrock-runtime.{region}.amazonaws.com`,
/// [`ProviderOptions::default`] (connect 30 s, idle 90 s) and
/// [`RetryPolicy::default`] (4 attempts).
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
#[derive(Clone, Debug)]
#[must_use]
pub struct BedrockBuilder {
    parts: ChannelParts,
}

impl BedrockBuilder {
    /// A builder with the defaults for `region`, authenticated by
    /// `credential`.
    pub(crate) fn new(credential: AuthMethod, region: String) -> Self {
        let mut parts =
            ChannelParts::new(format!("https://bedrock-runtime.{region}.amazonaws.com"));
        parts.kind = ChannelKind::Bedrock { region };
        parts.credential = Some(credential);

        Self { parts }
    }

    /// Sends requests to another origin, such as a proxy or a test server;
    /// the routes `/anthropic` and `/openai/v1` stay. A trailing `/` is
    /// ignored.
    pub fn base_url(mut self, origin: impl Into<String>) -> Self {
        self.parts.origin = origin.into().trim_end_matches('/').to_string();
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
    pub fn build(self) -> Bedrock {
        Bedrock::on(Channel::new(self.parts))
    }
}
