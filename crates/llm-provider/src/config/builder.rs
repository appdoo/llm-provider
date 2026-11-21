//! [`ModelBuilder`]: a model named by values, and its setters.

use std::marker::PhantomData;
use std::time::Duration;

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use super::Protocol;
use crate::provider::{AuthMethod, ProviderOptions, RetryPolicy};

/// A model named by values: protocol, endpoint, region, model, retry and
/// timeouts, and a credential set in code. `T` is the model it builds,
/// [`Llm`](crate::Llm) or [`Embedder`](crate::Embedder); start one with
/// [`Llm::builder`](crate::Llm::builder) or
/// [`Embedder::builder`](crate::Embedder::builder), or read one from a
/// configuration file.
///
/// The file keys are `protocol`, `endpoint`, `region`, `model`,
/// `max_attempts`, `connect_timeout_secs` and `stream_idle_timeout_secs`;
/// `default` reads as `protocol` and `base_url` as `endpoint`. No key holds
/// a credential, and serialising never writes one.
///
/// ```no_run
/// use llm_provider::{Llm, Protocol};
///
/// # fn run(key: String) -> Result<(), llm_provider::Error> {
/// let qwen = Llm::builder("qwen3")
///     .protocol(Protocol::OpenAi)
///     .endpoint("http://gpu-box:8000/v1")
///     .api_key(key)
///     .build()?;
/// # let _ = qwen; Ok(()) }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "")]
#[must_use]
pub struct ModelBuilder<T> {
    #[serde(default, alias = "default")]
    pub(super) protocol: Protocol,
    #[serde(default, alias = "base_url", skip_serializing_if = "Option::is_none")]
    pub(super) endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_attempts: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connect_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_idle_timeout_secs: Option<u64>,
    #[serde(skip)]
    pub(super) credential: Option<AuthMethod>,
    #[serde(skip)]
    builds: PhantomData<fn() -> T>,
}

impl<T> ModelBuilder<T> {
    /// A builder for `model` with every other value unset.
    pub(crate) fn new(model: String) -> Self {
        Self {
            protocol: Protocol::default(),
            endpoint: None,
            region: None,
            model: Some(model),
            max_attempts: None,
            connect_timeout_secs: None,
            stream_idle_timeout_secs: None,
            credential: None,
            builds: PhantomData,
        }
    }

    /// Sets the protocol; key `protocol`. Default [`Protocol::Anthropic`].
    pub fn protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Sets the address requests go to; key `endpoint`. The Anthropic API
    /// and Bedrock default to their public origins; an OpenAI-compatible
    /// server outside Bedrock needs one. The address is checked by
    /// [`build`](Self::build).
    pub fn endpoint(mut self, url: impl Into<String>) -> Self {
        self.endpoint = Some(url.into());
        self
    }

    /// Sends requests to Amazon Bedrock in `region`, such as
    /// `eu-central-1`; key `region`. The region is checked by
    /// [`build`](Self::build).
    pub fn bedrock(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Authenticates with an API key, [`AuthMethod::ApiKey`]. Replaces any
    /// credential set before.
    pub fn api_key(self, key: impl Into<SecretString>) -> Self {
        self.credential(AuthMethod::ApiKey(key.into()))
    }

    /// Authenticates with an OAuth access token, [`AuthMethod::Bearer`].
    /// Replaces any credential set before.
    pub fn oauth_token(self, token: impl Into<SecretString>) -> Self {
        self.credential(AuthMethod::Bearer(token.into()))
    }

    /// Authenticates with `credential`. Replaces any credential set before.
    pub fn credential(mut self, credential: AuthMethod) -> Self {
        self.credential = Some(credential);
        self
    }

    /// Sets the total attempts per request; 1 turns retry off. Default 4.
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = Some(n);
        self
    }

    /// Sets the TCP/TLS connect limit in seconds. Default 30.
    pub fn connect_timeout_secs(mut self, secs: u64) -> Self {
        self.connect_timeout_secs = Some(secs);
        self
    }

    /// Sets the longest silence on a response in seconds. Default 90.
    pub fn stream_idle_timeout_secs(mut self, secs: u64) -> Self {
        self.stream_idle_timeout_secs = Some(secs);
        self
    }

    /// The timeouts set here, with [`ProviderOptions::default`] for the
    /// unset ones.
    pub(super) fn options(&self) -> ProviderOptions {
        let mut options = ProviderOptions::default();
        if let Some(secs) = self.connect_timeout_secs {
            options = options.connect_timeout(Duration::from_secs(secs));
        }
        if let Some(secs) = self.stream_idle_timeout_secs {
            options = options.idle_gap(Duration::from_secs(secs));
        }

        options
    }

    /// The retry policy set here, with [`RetryPolicy::default`] for the
    /// unset parts.
    pub(super) fn retry_policy(&self) -> RetryPolicy {
        let mut policy = RetryPolicy::default();
        if let Some(n) = self.max_attempts {
            policy = policy.max_attempts(n);
        }

        policy
    }
}
