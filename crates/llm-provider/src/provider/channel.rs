//! [`Channel`]: the address, credential, connection pool and retry policy one
//! or more endpoints share.

use std::fmt;
#[cfg(test)]
use std::sync::Arc;

use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::options::ProviderOptions;
#[cfg(test)]
use crate::provider::retry::Sleeper;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::transport::Http;

/// The kind of service a [`Channel`] reaches, fixed when it is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChannelKind {
    /// A service that is what its origin says: the Anthropic API, the
    /// OpenAI API or an OpenAI-compatible server.
    Service,
    /// Amazon Bedrock in `region`, which is checked before every request.
    Bedrock {
        /// The AWS region in the origin's host name, such as `eu-central-1`.
        region: String,
    },
}

/// What a [`Channel`] is built from, filled in by an endpoint's builder.
#[derive(Clone)]
pub(crate) struct ChannelParts {
    /// The address requests go to, without a trailing `/`.
    pub(crate) origin: String,
    /// The kind of service at the origin.
    pub(crate) kind: ChannelKind,
    /// The credential to send, if the server wants one.
    pub(crate) credential: Option<AuthMethod>,
    /// The timeouts of the client built on first use.
    pub(crate) options: ProviderOptions,
    /// A client the caller configured, used instead of building one.
    pub(crate) client: Option<reqwest::Client>,
    /// The retry policy of every request.
    pub(crate) retry: RetryPolicy,
    /// What a test waits through between attempts.
    #[cfg(test)]
    pub(crate) sleeper: Option<Arc<dyn Sleeper>>,
}

impl ChannelParts {
    /// The parts of a [`ChannelKind::Service`] channel at `origin` with the
    /// defaults: no credential, [`ProviderOptions::default`] and
    /// [`RetryPolicy::default`].
    pub(crate) fn new(origin: String) -> Self {
        Self {
            origin,
            kind: ChannelKind::Service,
            credential: None,
            options: ProviderOptions::default(),
            client: None,
            retry: RetryPolicy::default(),
            #[cfg(test)]
            sleeper: None,
        }
    }
}

/// Hand-written because a test's sleeper has no `Debug`; the credential
/// prints masked, as [`AuthMethod`]'s `Debug` does.
impl fmt::Debug for ChannelParts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelParts")
            .field("origin", &self.origin)
            .field("kind", &self.kind)
            .field("credential", &self.credential)
            .field("options", &self.options)
            .field("client", &self.client)
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

/// The address, credential, connection pool and retry policy one or more
/// endpoints share.
pub(crate) struct Channel {
    origin: String,
    kind: ChannelKind,
    credential: Option<AuthMethod>,
    http: Http,
    retry: RetryPolicy,
    #[cfg(test)]
    sleeper: Option<Arc<dyn Sleeper>>,
}

impl Channel {
    /// The channel for `parts`. Its connection pool is built on first use
    /// unless `parts` carries a client.
    pub(crate) fn new(parts: ChannelParts) -> Self {
        let options = parts.options;
        let http = parts.client.map_or_else(|| Http::new(options), Http::from);

        Self {
            origin: parts.origin,
            kind: parts.kind,
            credential: parts.credential,
            http,
            retry: parts.retry,
            #[cfg(test)]
            sleeper: parts.sleeper,
        }
    }

    /// The address requests go to, without a trailing `/`.
    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    /// The credential to send, if any.
    pub(crate) fn credential(&self) -> Option<&AuthMethod> {
        self.credential.as_ref()
    }

    /// The connection pool every request goes through.
    pub(crate) fn http(&self) -> &Http {
        &self.http
    }

    /// True when the channel reaches Amazon Bedrock.
    pub(crate) fn is_bedrock(&self) -> bool {
        matches!(self.kind, ChannelKind::Bedrock { .. })
    }

    /// The Bedrock region, checked against `[a-z]{2}(-[a-z]+)+-\d`; an
    /// invalid one is [`Error::InvalidRegion`]. Any other kind passes.
    pub(crate) fn check_region(&self) -> Result<(), Error> {
        match &self.kind {
            ChannelKind::Bedrock { region } if !is_region(region) => {
                Err(Error::InvalidRegion(region.clone()))
            }
            _ => Ok(()),
        }
    }

    /// Wraps `api`, a protocol client's one-attempt calls, in this channel's
    /// retry policy.
    pub(crate) fn retrying<P>(&self, api: P) -> RetryingProvider<P> {
        let retrying = RetryingProvider::new(api, self.retry);
        #[cfg(test)]
        let retrying = match &self.sleeper {
            Some(sleeper) => retrying.with_sleeper(Arc::clone(sleeper)),
            None => retrying,
        };

        retrying
    }
}

/// Hand-written so that nothing but the origin is printed: no credential can
/// reach a log.
impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

/// True when `region` matches `[a-z]{2}(-[a-z]+)+-\d`, as `eu-central-1`
/// or `us-gov-west-1` do.
fn is_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    let [first, middle @ .., last] = parts.as_slice() else {
        return false;
    };
    let letters = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase());

    first.len() == 2
        && letters(first)
        && !middle.is_empty()
        && middle.iter().all(|part| letters(part))
        && last.len() == 1
        && last.bytes().all(|b| b.is_ascii_digit())
}
