//! [`Anthropic`]: an endpoint speaking the Messages API, which hands out one
//! [`Llm`] per model and, with the `batch` feature, [`Batches`](super::Batches).
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let haiku = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_HAIKU_5_5);
//! let answer = haiku.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

use std::fmt;
use std::sync::Arc;

use crate::provider::anthropic::api::AnthropicApi;
#[cfg(feature = "batch")]
use crate::provider::anthropic::batch::Batches;
use crate::provider::anthropic::builder::AnthropicBuilder;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::Route;
use crate::provider::llm::Llm;
use crate::provider::retry::RetryingProvider;

/// The Messages API origin requests go to unless
/// [`base_url`](AnthropicBuilder::base_url) names another.
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// An endpoint speaking the Anthropic Messages API, retrying transient
/// failures: the Anthropic API from [`new`](Self::new), or Claude on Bedrock
/// from [`Bedrock::anthropic`](crate::Bedrock::anthropic). Clones share one
/// HTTP connection pool, and so does every [`Llm`] it hands out.
#[derive(Clone)]
pub struct Anthropic {
    api: AnthropicApi,
    chat: Arc<RetryingProvider<AnthropicApi>>,
}

impl Anthropic {
    /// Claude Haiku 5.5 on the Anthropic API. A `Bedrock::*` id is valid on a
    /// Bedrock endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API;
    /// the other way round the server answers 400.
    pub const CLAUDE_HAIKU_5_5: &'static str = "claude-haiku-5-5";
    /// Claude Sonnet 5.5 on the Anthropic API. A `Bedrock::*` id is valid on
    /// a Bedrock endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API;
    /// the other way round the server answers 400.
    pub const CLAUDE_SONNET_5_5: &'static str = "claude-sonnet-5-5";
    /// Claude Opus 5.5 on the Anthropic API. A `Bedrock::*` id is valid on a
    /// Bedrock endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API;
    /// the other way round the server answers 400.
    pub const CLAUDE_OPUS_5_5: &'static str = "claude-opus-5-5";
    /// Claude Fable 5.1 on the Anthropic API. A `Bedrock::*` id is valid on
    /// a Bedrock endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API;
    /// the other way round the server answers 400.
    pub const CLAUDE_FABLE_5_1: &'static str = "claude-fable-5-1";

    /// The Anthropic API with `key` and the defaults. A plain string is an
    /// API key; pass an OAuth token as [`AuthMethod::Bearer`]. Use
    /// [`builder`](Self::builder) for another origin, timeouts or
    /// [retry policy](crate::RetryPolicy).
    ///
    /// ```no_run
    /// use llm_provider::{Anthropic, AuthMethod};
    ///
    /// let claude = Anthropic::new(AuthMethod::Bearer("sk-ant-oat01-…".into()));
    /// # let _ = claude;
    /// ```
    #[must_use]
    pub fn new(key: impl Into<AuthMethod>) -> Self {
        Self::builder(key).build()
    }

    /// A builder for the Anthropic API with another origin, timeouts or
    /// retry policy.
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use llm_provider::{Anthropic, ProviderOptions};
    ///
    /// let claude = Anthropic::builder("sk-ant-…")
    ///     .timeouts(ProviderOptions::default().connect_timeout(Duration::from_secs(5)))
    ///     .build();
    /// # let _ = claude;
    /// ```
    pub fn builder(key: impl Into<AuthMethod>) -> AnthropicBuilder {
        AnthropicBuilder::new(key.into())
    }

    /// The model `model` on this endpoint.
    ///
    /// ```no_run
    /// use llm_provider::Anthropic;
    ///
    /// let sonnet = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
    /// # let _ = sonnet;
    /// ```
    #[must_use]
    pub fn llm(&self, model: impl Into<String>) -> Llm {
        Llm::new(self.chat.clone(), model)
    }

    /// Message batches for `model` on this endpoint. On Bedrock every call
    /// is [`Error::Unsupported`](crate::Error::Unsupported), before any
    /// request.
    ///
    /// ```no_run
    /// use llm_provider::Anthropic;
    ///
    /// let batches = Anthropic::new("sk-ant-…").batches(Anthropic::CLAUDE_HAIKU_5_5);
    /// # let _ = batches;
    /// ```
    #[cfg(feature = "batch")]
    #[must_use]
    pub fn batches(&self, model: impl Into<String>) -> Batches {
        Batches::new(self.api.clone(), model.into())
    }

    /// The endpoint on `route`: the origin itself for the Anthropic API,
    /// `/anthropic` on Bedrock.
    pub(crate) fn routed(route: Route) -> Self {
        let api = AnthropicApi::new(route);
        let chat = api.route().channel().retrying(api.clone());

        Self {
            api,
            chat: Arc::new(chat),
        }
    }
}

/// Hand-written so the credential never reaches logs: shows the origin and
/// route only.
impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("base_url", &self.api.base_url())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
