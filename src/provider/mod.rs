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
mod ask;
mod auth;
mod batch;
mod error;
mod event;
mod openai;
mod retry;
#[cfg(test)]
mod test_stub;
mod traits;

use std::pin::Pin;
use std::time::Duration;

use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use tokio_stream::Stream;

pub use anthropic::{Anthropic, AnthropicBuilder};
pub use ask::Request;
pub use auth::AuthMethod;
pub use batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
pub use error::{ApiError, Error, ErrorKind, Oversize, RequestError};
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

/// Which provider to build and how: model, origin, timeouts and retry.
/// Unset values take each provider's defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// The provider: `"claude"` or `"anthropic"` for Anthropic, `"openai"`
    /// for an OpenAI-compatible server. Default `"claude"`.
    pub default: String,
    /// The model id; Anthropic defaults to `claude-sonnet-4-6`, OpenAI needs one.
    pub model: Option<String>,
    /// The API origin; Anthropic defaults to `https://api.anthropic.com`,
    /// OpenAI needs one.
    pub base_url: Option<String>,
    /// Total attempts per request; 1 turns retry off. Default 4.
    pub max_attempts: Option<u32>,
    /// TCP/TLS connect limit in seconds. Default 30.
    pub connect_timeout_secs: Option<u64>,
    /// Longest silence on a response in seconds. Default 90.
    pub stream_idle_timeout_secs: Option<u64>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            default: "claude".to_string(),
            model: None,
            base_url: None,
            max_attempts: None,
            connect_timeout_secs: None,
            stream_idle_timeout_secs: None,
        }
    }
}

impl ProviderConfig {
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

/// The provider `config.default` names, with retry on. Anthropic needs
/// `auth`, else [`Error::MissingCredentials`]; an OpenAI-compatible server
/// needs `base_url` and `model` and sends `auth` as a bearer key when given.
///
/// # Errors
/// [`Error::MissingCredentials`], [`Error::UnknownProvider`], or
/// [`Error::InvalidRequest`] for an incomplete OpenAI configuration.
pub fn create_provider(
    config: &ProviderConfig,
    auth: Option<AuthMethod>,
) -> Result<Box<dyn LlmProvider>, Error> {
    match config.default.as_str() {
        "claude" | "anthropic" => {
            let auth = auth.ok_or_else(|| Error::MissingCredentials {
                message: "anthropic provider needs a credential".to_string(),
            })?;
            Ok(Box::new(create_anthropic(config, auth)))
        }
        "openai" => {
            let api_key = auth.map(|auth| match auth {
                AuthMethod::ApiKey(key) | AuthMethod::Bearer(key) => {
                    key.expose_secret().to_string()
                }
            });
            Ok(Box::new(create_openai(config, api_key)?))
        }
        other => Err(Error::UnknownProvider(other.to_string())),
    }
}

/// An Anthropic client from `config`, keeping [`BatchProvider`]; ignores
/// `config.default`. Assembled step by step on an [`AnthropicBuilder`].
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
) -> Result<OpenAiCompatible, Error> {
    let base_url = config
        .base_url
        .as_deref()
        .ok_or(RequestError::MissingBaseUrl { provider: "openai" })?;
    let model = config
        .model
        .as_deref()
        .ok_or(RequestError::MissingModel { provider: "openai" })?;
    let api = OpenAiChatApi::with_options(base_url, model, api_key, config.options())?;
    Ok(OpenAiCompatible::from_transport(api, config.retry_policy()))
}

#[cfg(test)]
mod tests;
