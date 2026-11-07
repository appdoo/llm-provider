//! The shared HTTP client, the one place a response's status is checked and
//! the one place a `reqwest` error becomes an [`Error`].

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::sync::OnceCell;

use crate::provider::{Error, ProviderOptions};
use crate::transport::ByteStream;

/// The `reqwest` client every provider sends through. `connect_timeout`
/// bounds the TCP/TLS handshake; `read_timeout` is the idle gap: it bounds the
/// wait for the response headers and then each body read, resetting after
/// every successful read. No redirects are followed, so a credential never
/// travels to a second origin.
pub(crate) fn build_client(options: &ProviderOptions) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(options.connect_timeout)
        .read_timeout(options.idle_gap)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// The [`Error`] for a `reqwest` failure: a timeout after the connection
/// stood is the idle gap running out, [`Error::IdleTimeout`]; everything
/// else, a connect timeout included, is [`Error::Http`].
pub(crate) fn from_reqwest(error: reqwest::Error, idle_gap: Duration) -> Error {
    if error.is_timeout() && !error.is_connect() {
        Error::IdleTimeout { after: idle_gap }
    } else {
        Error::Http(error)
    }
}

/// The HTTP connection pool of one provider client, with its timeouts.
/// Clones share one `reqwest::Client`, built on first use.
#[derive(Clone, Debug)]
pub(crate) struct Http {
    options: ProviderOptions,
    client: Arc<OnceCell<reqwest::Client>>,
}

impl Http {
    /// A pool that builds its client from `options` on first use.
    pub(crate) fn new(options: ProviderOptions) -> Self {
        Self {
            options,
            client: Arc::new(OnceCell::new()),
        }
    }

    /// The `reqwest` client, built by [`build_client`] on first use.
    pub(crate) async fn client(&self) -> Result<&reqwest::Client, Error> {
        self.client
            .get_or_try_init(|| async { build_client(&self.options).map_err(Error::Http) })
            .await
    }

    /// The longest silence allowed before the headers and between body chunks.
    pub(crate) fn idle_gap(&self) -> Duration {
        self.options.idle_gap
    }

    /// Sends `request`. A non-2xx answer becomes [`Error::Api`], built from
    /// its status, headers and at most
    /// [`ERROR_BODY_CAP`](crate::transport::ERROR_BODY_CAP) bytes of its body.
    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        todo!()
    }

    /// Sends `request` and returns its body as byte chunks; a read timeout
    /// ends the stream with [`Error::IdleTimeout`].
    pub(crate) async fn byte_stream(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<ByteStream, Error> {
        todo!()
    }

    /// Sends `request` and parses its whole body as `T`.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        todo!()
    }
}
