//! [`Protocol`]: the wire format a model is spoken to in.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::ConfigError;

/// The wire format a model is spoken to in. In a configuration file and on
/// the command line it is `"anthropic"` (or `"claude"`) or `"openai"`.
///
/// ```
/// use llm_provider::Protocol;
///
/// let protocol: Protocol = "openai".parse().expect("a known protocol");
/// assert_eq!(protocol, Protocol::OpenAi);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Protocol {
    /// The Anthropic Messages API.
    #[default]
    #[serde(alias = "claude")]
    Anthropic,
    /// The OpenAI Chat Completions API, and `/embeddings` for embedding
    /// models.
    OpenAi,
}

/// Hand-written because the name is matched without regard to case, which
/// `serde` does not do.
impl FromStr for Protocol {
    type Err = ConfigError;

    /// The protocol named `name`, in any case; `claude` is
    /// [`Anthropic`](Protocol::Anthropic).
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name.to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Ok(Protocol::Anthropic),
            "openai" => Ok(Protocol::OpenAi),
            _ => Err(ConfigError::UnknownProtocol {
                name: name.to_string(),
            }),
        }
    }
}
