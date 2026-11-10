//! Providers built from a configuration file: a [`ProviderConfig`] read
//! through `serde` names the provider, model, origin, timeouts and retry, and
//! [`create_provider`] builds it. Code that knows its provider builds
//! [`Anthropic`] or [`OpenAi`] directly instead.
//!
//! ```no_run
//! use llm_provider::config::{create_provider, ProviderConfig};
//! use llm_provider::AuthMethod;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let config: ProviderConfig = serde_json::from_str(r#"{"default":"claude"}"#)?;
//! let provider = create_provider(&config, Some(AuthMethod::from("sk-ant-…")))?;
//! let answer = provider.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::provider::{
    Anthropic, AuthMethod, Error, ErrorKind, Llm, OpenAi, ProviderOptions, RetryPolicy,
};

/// The Anthropic model [`create_provider`] uses when the configuration names
/// none.
const DEFAULT_ANTHROPIC_MODEL: &str = Anthropic::CLAUDE_SONNET_5_5;

/// A provider configuration the factory cannot build from.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The provider needs a credential and none was given.
    #[error("{provider} provider needs a credential")]
    MissingCredential {
        /// The configured provider's name.
        provider: &'static str,
    },
    /// The provider needs a `base_url` and the configuration has none.
    #[error("{provider} provider needs base_url")]
    MissingBaseUrl {
        /// The configured provider's name.
        provider: &'static str,
    },
    /// The provider needs a `model` and the configuration has none.
    #[error("{provider} provider needs model")]
    MissingModel {
        /// The configured provider's name.
        provider: &'static str,
    },
}

impl ConfigError {
    /// The class of this error: a missing credential is
    /// [`ErrorKind::Auth`], a missing setting [`ErrorKind::InvalidRequest`].
    pub(crate) fn kind(&self) -> ErrorKind {
        match self {
            ConfigError::MissingCredential { .. } => ErrorKind::Auth,
            ConfigError::MissingBaseUrl { .. } | ConfigError::MissingModel { .. } => {
                ErrorKind::InvalidRequest
            }
        }
    }
}

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
/// use llm_provider::config::{ProviderConfig, ProviderKind};
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

    /// Sets the model id; Anthropic defaults to
    /// [`Anthropic::CLAUDE_SONNET_5_5`], OpenAI needs one.
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

/// The model `config` names, with retry on. Anthropic needs `auth`; an
/// OpenAI-compatible server needs `base_url` and `model` and sends `auth`
/// as a bearer key when given.
///
/// # Errors
/// [`Error::Config`] for a missing credential, `base_url` or `model`.
pub fn create_provider(config: &ProviderConfig, auth: Option<AuthMethod>) -> Result<Llm, Error> {
    match config.provider {
        ProviderKind::Claude => {
            let auth = auth.ok_or(ConfigError::MissingCredential {
                provider: "anthropic",
            })?;
            Ok(create_anthropic(config, auth))
        }
        ProviderKind::OpenAi => Ok(create_openai(config, auth)?),
    }
}

/// A model on the Anthropic API from `config`, [`DEFAULT_ANTHROPIC_MODEL`]
/// when it names none.
fn create_anthropic(config: &ProviderConfig, auth: AuthMethod) -> Llm {
    let model = config.model.as_deref().unwrap_or(DEFAULT_ANTHROPIC_MODEL);
    let mut builder = Anthropic::builder(auth)
        .timeouts(config.options())
        .retry(config.retry_policy());
    if let Some(url) = &config.base_url {
        builder = builder.base_url(url);
    }

    builder.build().llm(model)
}

/// A model on an OpenAI-compatible server from `config`. `base_url` and
/// `model` are required so a local setup never falls through to a hosted API.
fn create_openai(config: &ProviderConfig, auth: Option<AuthMethod>) -> Result<Llm, ConfigError> {
    let base_url = config
        .base_url
        .as_deref()
        .ok_or(ConfigError::MissingBaseUrl { provider: "openai" })?;
    let model = config
        .model
        .as_deref()
        .ok_or(ConfigError::MissingModel { provider: "openai" })?;

    let mut builder = OpenAi::builder(base_url)
        .timeouts(config.options())
        .retry(config.retry_policy());
    if let Some(auth) = auth {
        builder = builder.credential(auth);
    }

    Ok(builder.build().llm(model))
}

#[cfg(test)]
mod tests;
