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
        todo!()
    }
}

impl ProviderConfig {
    /// The timeouts this configuration sets, with
    /// [`ProviderOptions::default`] for the unset ones.
    #[must_use]
    pub fn options(&self) -> ProviderOptions {
        todo!()
    }

    /// The retry policy this configuration sets, with
    /// [`RetryPolicy::default`] for the unset parts.
    #[must_use]
    pub fn retry_policy(&self) -> RetryPolicy {
        todo!()
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
    todo!()
}

/// An Anthropic client from `config`, keeping [`BatchProvider`]; ignores
/// `config.default`. Assembled step by step on an [`AnthropicBuilder`].
#[must_use]
pub fn create_anthropic(config: &ProviderConfig, auth: AuthMethod) -> Anthropic {
    todo!()
}

/// An OpenAI-compatible client from `config`. `base_url` and `model` are
/// required so a local setup never falls through to a hosted API.
fn create_openai(
    config: &ProviderConfig,
    api_key: Option<String>,
) -> Result<OpenAiCompatible, Error> {
    todo!()
}

#[cfg(test)]
mod tests;
