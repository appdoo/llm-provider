//! [`ConfigError`]: a model description [`build`](super::ModelBuilder::build)
//! cannot build from.

use crate::provider::ErrorKind;

/// A model description that cannot be built: a missing credential,
/// endpoint or model, or an unknown protocol name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The endpoint needs a credential and none was given.
    #[error("the {endpoint} endpoint needs a credential")]
    MissingCredential {
        /// The endpoint's name: `anthropic` or `bedrock`.
        endpoint: &'static str,
    },
    /// The endpoint needs an address and none was given.
    #[error("the {endpoint} endpoint needs an address")]
    MissingEndpoint {
        /// The endpoint's name.
        endpoint: &'static str,
    },
    /// The endpoint needs a model id and none was given.
    #[error("the {endpoint} endpoint needs a model")]
    MissingModel {
        /// The endpoint's name.
        endpoint: &'static str,
    },
    /// A protocol name that is neither `anthropic` nor `openai`.
    #[error("unknown protocol {name:?}; expected anthropic or openai")]
    UnknownProtocol {
        /// The name as given.
        name: String,
    },
    /// A Bedrock region not of the form `eu-central-1`.
    #[error("invalid Bedrock region {0:?}: expected a name such as \"eu-central-1\"")]
    InvalidRegion(String),
    /// An endpoint address that does not parse as an absolute URL.
    #[error("invalid endpoint URL")]
    InvalidOrigin(#[source] url::ParseError),
    /// An endpoint address whose scheme is not `http` or `https`.
    #[error("unsupported endpoint URL scheme {scheme:?}; expected http or https")]
    UnsupportedScheme {
        /// The scheme as parsed, such as `ftp`.
        scheme: String,
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
            | ConfigError::UnknownProtocol { .. }
            | ConfigError::InvalidRegion(_)
            | ConfigError::InvalidOrigin(_)
            | ConfigError::UnsupportedScheme { .. } => ErrorKind::InvalidRequest,
        }
    }
}
