//! [`ConfigError`]: a model description [`build`](super::ModelBuilder::build)
//! cannot build from.

use crate::provider::ErrorKind;

/// A model description that cannot be built: a missing credential,
/// endpoint or model, or an unknown protocol name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The endpoint needs a credential and none was given.
    #[error("{provider} provider needs a credential")]
    MissingCredential {
        /// The endpoint's name: `anthropic` or `bedrock`.
        provider: &'static str,
    },
    /// The endpoint needs an address and none was given.
    #[error("{provider} provider needs an endpoint")]
    MissingEndpoint {
        /// The endpoint's name.
        provider: &'static str,
    },
    /// The endpoint needs a model id and none was given.
    #[error("{provider} provider needs model")]
    MissingModel {
        /// The endpoint's name.
        provider: &'static str,
    },
    /// A protocol name that is neither `anthropic` nor `openai`.
    #[error("unknown protocol {name:?}; expected anthropic or openai")]
    UnknownProtocol {
        /// The name as given.
        name: String,
    },
}

impl ConfigError {
    /// The class of this error: a missing credential is
    /// [`ErrorKind::Auth`], every other one [`ErrorKind::InvalidRequest`].
    pub(crate) fn kind(&self) -> ErrorKind {
        match self {
            ConfigError::MissingCredential { .. } => ErrorKind::Auth,
            ConfigError::MissingEndpoint { .. }
            | ConfigError::MissingModel { .. }
            | ConfigError::UnknownProtocol { .. } => ErrorKind::InvalidRequest,
        }
    }
}
