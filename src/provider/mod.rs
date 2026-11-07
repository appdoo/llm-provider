//! The provider layer: the [`LlmProvider`] and [`BatchProvider`] traits, the
//! [`Anthropic`] and [`OpenAiCompatible`] clients, retry, and factories that
//! build a provider from a [`ProviderConfig`].
//!
//! ```no_run
//! use llm_provider::provider::{create_provider, AuthMethod, ProviderConfig};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let provider = create_provider(&ProviderConfig::default(), Some(AuthMethod::from("sk-ant-…")))?;
//! let answer = provider.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod anthropic;
mod api_error;
mod ask;
mod auth;
mod batch;
mod error;
mod event;
mod openai;
mod retry;
#[cfg(test)]
pub(crate) mod test_stub;
mod traits;

use std::pin::Pin;
use std::time::Duration;

use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

pub use anthropic::{Anthropic, AnthropicBuilder};
pub use api_error::ApiError;
pub use ask::Request;
pub use auth::AuthMethod;
pub use batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
pub use error::{ConfigError, Error, ErrorKind, Oversize, RequestError};
pub use event::{StopReason, StreamEvent};
use openai::OpenAiChatApi;
pub use openai::OpenAiCompatible;
pub use retry::RetryPolicy;
pub use traits::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, Completion, CompletionRequest, Content,
    LlmProvider, Message, MessageRole, ModelSelection, ProviderInfo, ProviderOptions,
    ToolDefinition, ToolUseCall, Usage,
};

/// The stream of events a provider returns for one request; the first error
/// ends it.
pub(crate) type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;

/// The provider a [`ProviderConfig`] builds; in a configuration file
/// `"claude"` (or `"anthropic"`) and `"openai"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ProviderKind {
    /// The Anthropic Messages API.
    #[default]
    #[serde(alias = "anthropic")]
    Claude,
    /// An OpenAI-compatible Chat Completions server.
    OpenAi,
}

/// Which provider to build and how: model, origin, timeouts and retry.
/// Unset values take each provider's defaults. It reads from a
/// configuration file through `serde`, or is assembled with its setters.
///
/// ```no_run
/// use llm_provider::provider::{ProviderConfig, ProviderKind};
///
/// let local = ProviderConfig::default()
///     .provider(ProviderKind::OpenAi)
///     .base_url("http://localhost:11434/v1")
///     .model("qwen3");
/// # let _ = local;
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
#[must_use]
pub struct ProviderConfig {
    #[serde(rename = "default")]
    provider: ProviderKind,
    model: Option<String>,
    base_url: Option<String>,
    max_attempts: Option<u32>,
    connect_timeout_secs: Option<u64>,
    stream_idle_timeout_secs: Option<u64>,
}

impl ProviderConfig {
    /// Sets the provider; key `default` in a configuration file. Default
    /// [`ProviderKind::Claude`].
    pub fn provider(mut self, kind: ProviderKind) -> Self {
        self.provider = kind;
        self
    }

    /// Sets the model id; Anthropic defaults to `claude-sonnet-4-6`, OpenAI
    /// needs one.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the API origin; Anthropic defaults to
    /// `https://api.anthropic.com`, OpenAI needs one.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
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

    /// The timeouts this configuration sets, with
    /// [`ProviderOptions::default`] for the unset ones.
    #[must_use]
    pub fn options(&self) -> ProviderOptions {
        let mut options = ProviderOptions::default();
        if let Some(secs) = self.connect_timeout_secs {
            options = options.connect_timeout(Duration::from_secs(secs));
        }
        if let Some(secs) = self.stream_idle_timeout_secs {
            options = options.idle_gap(Duration::from_secs(secs));
        }

        options
    }

    /// The retry policy this configuration sets, with
    /// [`RetryPolicy::default`] for the unset parts.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        let mut policy = RetryPolicy::default();
        if let Some(n) = self.max_attempts {
            policy = policy.max_attempts(n);
        }

        policy
    }
}

/// The provider `config` names, with retry on. Anthropic needs `auth`; an
/// OpenAI-compatible server needs `base_url` and `model` and sends `auth`
/// as a bearer key when given.
///
/// # Errors
/// [`Error::Config`] for a missing credential, `base_url` or `model`.
pub fn create_provider(
    config: &ProviderConfig,
    auth: Option<AuthMethod>,
) -> Result<Box<dyn LlmProvider>, Error> {
    match config.provider {
        ProviderKind::Claude => {
            let auth = auth.ok_or(ConfigError::MissingCredential {
                provider: "anthropic",
            })?;
            Ok(Box::new(create_anthropic(config, auth)))
        }
        ProviderKind::OpenAi => {
            let api_key = auth.map(|auth| match auth {
                AuthMethod::ApiKey(key) | AuthMethod::Bearer(key) => {
                    key.expose_secret().to_string()
                }
            });
            Ok(Box::new(create_openai(config, api_key)?))
        }
    }
}

/// An Anthropic client from `config`, keeping [`BatchProvider`]; ignores
/// the configured provider kind.
#[must_use]
pub fn create_anthropic(config: &ProviderConfig, auth: AuthMethod) -> Anthropic {
    let model = config.model.as_deref().unwrap_or(anthropic::DEFAULT_MODEL);
    let mut builder = Anthropic::builder(auth, model)
        .timeouts(config.options())
        .retry(config.retry_policy());
    if let Some(url) = &config.base_url {
        builder = builder.base_url(url);
    }

    builder.build()
}

/// An OpenAI-compatible client from `config`. `base_url` and `model` are
/// required so a local setup never falls through to a hosted API.
fn create_openai(
    config: &ProviderConfig,
    api_key: Option<String>,
) -> Result<OpenAiCompatible, ConfigError> {
    let base_url = config
        .base_url
        .as_deref()
        .ok_or(ConfigError::MissingBaseUrl { provider: "openai" })?;
    let model = config
        .model
        .as_deref()
        .ok_or(ConfigError::MissingModel { provider: "openai" })?;

    let api = OpenAiChatApi::with_options(base_url, model, api_key, config.options());
    Ok(OpenAiCompatible::from_transport(api, config.retry_policy()))
}

#[cfg(test)]
mod tests;
