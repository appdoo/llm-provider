//! [`Channel`]: the address, credential, connection pool and retry policy one
//! or more endpoints share, and the [`Route`] a protocol's paths go under.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use reqwest::{Method, RequestBuilder};
use tokio_stream::StreamExt;

use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::options::ProviderOptions;
use crate::provider::retry::{RetryPolicy, Retrying, Sleeper, TokioSleeper};
use crate::provider::EventStream;
use crate::transport::{ByteStream, Http};

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
#[derive(Clone, Debug)]
pub(crate) struct ChannelParts {
    /// The address requests go to, without a trailing `/`; set through
    /// [`set_origin`](Self::set_origin).
    origin: String,
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
    pub(crate) fn new(origin: &str) -> Self {
        let mut parts = Self {
            origin: String::new(),
            kind: ChannelKind::Service,
            credential: None,
            options: ProviderOptions::default(),
            client: None,
            retry: RetryPolicy::default(),
            sleeper: Arc::new(TokioSleeper),
        };
        parts.set_origin(origin);

        parts
    }

    /// Sends requests to `origin`. A trailing `/` is dropped here, the one
    /// place an origin is normalised.
    pub(crate) fn set_origin(&mut self, origin: &str) {
        origin.trim_end_matches('/').clone_into(&mut self.origin);
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

    /// Wraps `api`, a protocol's one-attempt calls, in this channel's retry
    /// policy.
    pub(crate) fn retrying<A>(&self, api: A) -> Retrying<A> {
        Retrying::new(api, self.retry, Arc::clone(&self.sleeper))
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

    /// A `method` request to `path` below the route, without credentials;
    /// an invalid Bedrock region fails here, before any I/O.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, Error> {
        self.channel.check_region()?;
        let url = format!("{}{path}", self.base_url());

        Ok(self.channel.http().client().await?.request(method, url))
    }

    /// The channel requests go through.
    pub(crate) fn channel(&self) -> &Channel {
        &self.channel
    }
}

/// The events of the response `open` yields, decoded by `decode`, as one
/// [`EventStream`]; an error opening it is the stream's only item.
pub(crate) fn decoded<F>(open: F, decode: fn(ByteStream) -> EventStream) -> EventStream
where
    F: Future<Output = Result<ByteStream, Error>> + Send + 'static,
{
    Box::pin(async_stream::try_stream! {
        let mut events = decode(open.await?);
        while let Some(event) = events.next().await {
            yield event?;
        }
    })
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
