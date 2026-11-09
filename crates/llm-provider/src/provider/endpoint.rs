//! [`Endpoint`]: where a protocol client sends its requests and what every
//! client at that address shares.

use std::fmt;

use crate::provider::auth::AuthMethod;
use crate::provider::options::ProviderOptions;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::transport::Http;

/// What an [`Endpoint`] is built from, filled in by a client's builder.
#[derive(Clone, Debug)]
pub(crate) struct EndpointParts {
    /// The address requests go to, without a trailing `/`.
    pub(crate) origin: String,
    /// The credential to send, if the server wants one.
    pub(crate) credential: Option<AuthMethod>,
    /// The timeouts of the client built on first use.
    pub(crate) options: ProviderOptions,
    /// A client the caller configured, used instead of building one.
    pub(crate) client: Option<reqwest::Client>,
    /// The retry policy of every request.
    pub(crate) retry: RetryPolicy,
}

impl EndpointParts {
    /// The parts of an endpoint at `origin` with the defaults: no credential,
    /// [`ProviderOptions::default`] and [`RetryPolicy::default`].
    pub(crate) fn new(origin: String) -> Self {
        Self {
            origin,
            credential: None,
            options: ProviderOptions::default(),
            client: None,
            retry: RetryPolicy::default(),
        }
    }
}

/// An address serving model APIs, with the credential, connection pool and
/// retry policy of every request sent there.
pub(crate) struct Endpoint {
    origin: String,
    credential: Option<AuthMethod>,
    http: Http,
    retry: RetryPolicy,
}

impl Endpoint {
    /// The endpoint for `parts`. Its connection pool is built on first use
    /// unless `parts` carries a client.
    pub(crate) fn new(parts: EndpointParts) -> Self {
        let options = parts.options;
        let http = parts.client.map_or_else(|| Http::new(options), Http::from);

        Self {
            origin: parts.origin,
            credential: parts.credential,
            http,
            retry: parts.retry,
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

    /// Wraps `api`, a protocol client's one-attempt calls, in this endpoint's
    /// retry policy.
    pub(crate) fn retrying<P>(&self, api: P) -> RetryingProvider<P> {
        RetryingProvider::new(api, self.retry)
    }
}

/// Hand-written so that nothing but the origin is printed: no credential can
/// reach a log.
impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}
