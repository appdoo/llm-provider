//! [`Channel`]: the address, credential, connection pool and retry policy one
//! or more endpoints share, and the [`Route`] a protocol's paths go under.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use reqwest::{Method, RequestBuilder};

use crate::config::ConfigError;
use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::options::ProviderOptions;
use crate::provider::retry::{retried, run, RetryPolicy, Sleeper, TokioSleeper};
use crate::provider::EventStream;
use crate::transport::Http;

/// The kind of service a [`Channel`] reaches, fixed when it is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChannelKind {
    /// A service that is what its origin says: the Anthropic API, the
    /// OpenAI API or an OpenAI-compatible server.
    Service,
    /// Amazon Bedrock; the region is in the origin's host name and was
    /// checked when built.
    Bedrock,
}

/// What a [`Channel`] is built from, filled in by an endpoint's builder.
#[derive(Clone, Debug)]
pub(crate) struct ChannelParts {
    /// The address requests go to; set through
    /// [`set_origin`](Self::set_origin).
    origin: reqwest::Url,
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
    /// What retry waits through between attempts: [`TokioSleeper`] unless a
    /// test sets a recorder.
    pub(crate) sleeper: Arc<dyn Sleeper>,
}

impl ChannelParts {
    /// The parts of a [`ChannelKind::Service`] channel at `origin` with the
    /// defaults: no credential, [`ProviderOptions::default`] and
    /// [`RetryPolicy::default`].
    pub(crate) fn new(origin: reqwest::Url) -> Self {
        Self {
            origin,
            kind: ChannelKind::Service,
            credential: None,
            options: ProviderOptions::default(),
            client: None,
            retry: RetryPolicy::default(),
            sleeper: Arc::new(TokioSleeper),
        }
    }

    /// Sends requests to `origin`.
    pub(crate) fn set_origin(&mut self, origin: reqwest::Url) {
        self.origin = origin;
    }
}

/// `text` parsed as an endpoint origin: an absolute `http` or `https` URL.
///
/// # Errors
///
/// [`ConfigError::InvalidOrigin`] when `text` does not parse as an absolute
/// URL, [`ConfigError::UnsupportedScheme`] when its scheme is neither `http`
/// nor `https`. Neither error carries `text`, whose userinfo may hold a
/// password.
pub(crate) fn parse_origin(text: &str) -> Result<reqwest::Url, ConfigError> {
    let origin = reqwest::Url::parse(text).map_err(ConfigError::InvalidOrigin)?;
    if !matches!(origin.scheme(), "http" | "https") {
        return Err(ConfigError::UnsupportedScheme {
            scheme: origin.scheme().to_string(),
        });
    }

    Ok(origin)
}

/// `text`, an origin this crate writes itself (the Anthropic and OpenAI
/// defaults, the Bedrock host of a checked region), as a URL.
///
/// Invariant: every such text is an absolute `https` URL, so the parse
/// cannot fail.
pub(crate) fn fixed_origin(text: &str) -> reqwest::Url {
    reqwest::Url::parse(text).expect("a crate-built origin is a valid URL")
}

/// The address, credential, connection pool and retry policy one or more
/// endpoints share.
pub(crate) struct Channel {
    origin: reqwest::Url,
    kind: ChannelKind,
    credential: Option<AuthMethod>,
    http: Http,
    retry: RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
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
            sleeper: parts.sleeper,
        }
    }

    /// The address requests go to, without a trailing `/`. A trailing `/`
    /// is dropped here, the one place an origin is normalised.
    pub(crate) fn origin(&self) -> &str {
        self.origin.as_str().trim_end_matches('/')
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
        matches!(self.kind, ChannelKind::Bedrock)
    }

    /// The stream `start` opens, retried under this channel's policy while
    /// only `Usage` has arrived.
    pub(crate) fn retried<F>(&self, start: F) -> EventStream
    where
        F: Fn() -> EventStream + Send + Sync + 'static,
    {
        retried(self.retry, Arc::clone(&self.sleeper), start)
    }

    /// The result of the one-shot call `attempt`, repeated under this
    /// channel's policy while `retry_if` holds for its error.
    pub(crate) async fn call<T, F, Fut>(
        &self,
        retry_if: fn(&Error) -> bool,
        attempt: F,
    ) -> Result<T, Error>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        run(&self.retry, Arc::clone(&self.sleeper), retry_if, attempt).await
    }
}

/// Hand-written so that nothing but the origin is printed: no credential can
/// reach a log.
impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel")
            .field("origin", &self.origin())
            .finish_non_exhaustive()
    }
}

/// A [`Channel`] under one route: where a protocol's paths are appended.
/// Clones share the channel.
#[derive(Clone, Debug)]
pub(crate) struct Route {
    channel: Arc<Channel>,
    prefix: &'static str,
}

impl Route {
    /// The channel's origin itself, as on the Anthropic API or an OpenAI
    /// server.
    pub(crate) fn root(channel: Arc<Channel>) -> Self {
        Self::under(channel, "")
    }

    /// `prefix` below the channel's origin, such as Bedrock's `/anthropic`.
    pub(crate) fn under(channel: Arc<Channel>, prefix: &'static str) -> Self {
        Self { channel, prefix }
    }

    /// The origin and the route prefix, without a trailing `/`.
    pub(crate) fn base_url(&self) -> String {
        format!("{}{}", self.channel.origin(), self.prefix)
    }

    /// A `method` request to `path` below the route, without credentials.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, Error> {
        let url = format!("{}{path}", self.base_url());

        Ok(self.channel.http().client().await?.request(method, url))
    }

    /// The channel requests go through.
    pub(crate) fn channel(&self) -> &Channel {
        &self.channel
    }

    /// `Err(Error::Unsupported(what))` on a Bedrock channel, before any I/O;
    /// `Ok(())` elsewhere. `what` is the whole message.
    pub(crate) fn refuse_on_bedrock(&self, what: &'static str) -> Result<(), Error> {
        if self.channel.is_bedrock() {
            return Err(Error::Unsupported(what));
        }

        Ok(())
    }
}
