//! [`Transport`]: where requests go, the headers they carry and the retry
//! that guards their streams.

use std::sync::Arc;

use reqwest::{Method, RequestBuilder};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use super::ByteStream;
use crate::config::ConfigError;
use crate::provider::channel::{parse_origin, Channel, ChannelParts, Route};
use crate::provider::{Error, EventStream};

/// An address, its headers, a connection pool and a retry policy: what a
/// provider needs to send a request body and read the answer.
///
/// Clones share the connection pool.
///
/// ```no_run
/// use llm_provider::core::Transport;
///
/// # fn run() -> Result<(), llm_provider::config::ConfigError> {
/// let transport = Transport::at("http://localhost:11434")?.bearer("secret");
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct Transport {
    route: Route,
    headers: Vec<(String, SecretString)>,
}

impl Transport {
    /// A transport to the server at `url`, an absolute `http` or `https`
    /// URL, with the default timeouts and retry policy and no headers.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] when `url` is not an absolute `http` or `https` URL.
    pub fn at(url: &str) -> Result<Self, ConfigError> {
        let parts = ChannelParts::new(parse_origin(url)?);

        Ok(Self::on(Route::root(Arc::new(Channel::new(parts)))))
    }

    /// A transport on `route`, which already holds the pool and the policy.
    pub(crate) fn on(route: Route) -> Self {
        Self {
            route,
            headers: Vec::new(),
        }
    }

    /// Sends `Authorization: Bearer {token}` with every request.
    #[must_use]
    pub fn bearer(self, token: impl Into<SecretString>) -> Self {
        let token = token.into();

        self.header("authorization", format!("Bearer {}", token.expose_secret()))
    }

    /// Sends the header `name: value` with every request. The value is
    /// treated as a secret and never printed.
    #[must_use]
    pub fn header(mut self, name: impl Into<String>, value: impl Into<SecretString>) -> Self {
        self.headers.push((name.into(), value.into()));

        self
    }

    /// The route requests go to.
    pub(crate) fn route(&self) -> &Route {
        &self.route
    }

    /// A `method` request to `path` below the address, with the headers
    /// set.
    pub(crate) async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, Error> {
        let mut request = self.route.request(method, path).await?;
        for (name, value) in &self.headers {
            request = request.header(name, value.expose_secret());
        }

        Ok(request)
    }

    /// POSTs `body` as JSON to `path` and decodes the reply with `decode`,
    /// as one stream of events. An error before the first content event is
    /// retried under the retry policy; a failure to send is the stream's
    /// only item.
    ///
    /// `decode` is one of the [`decode`](super::decode) functions or your
    /// own, such as one built on [`decode::lines`](super::decode::lines).
    pub fn send_body<D>(&self, path: &str, body: Value, decode: D) -> EventStream
    where
        D: Fn(ByteStream) -> EventStream + Send + Sync + 'static,
    {
        let (transport, path, decode) = (self.clone(), path.to_owned(), Arc::new(decode));

        self.route.channel().retried(move || {
            let (transport, path, body) = (transport.clone(), path.clone(), body.clone());
            let decode = Arc::clone(&decode);

            Box::pin(async_stream::try_stream! {
                let post = transport.request(Method::POST, &path).await?.json(&body);
                let bytes = transport.route.channel().http().byte_stream(post).await?;
                let mut events = decode(bytes);
                while let Some(event) = tokio_stream::StreamExt::next(&mut events).await {
                    yield event?;
                }
            })
        })
    }
}
