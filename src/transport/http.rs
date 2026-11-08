//! The shared HTTP client: the one place a response's status is checked and
//! a `reqwest` error becomes an [`Error`].

use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "batch")]
use serde::de::DeserializeOwned;
use tokio::sync::OnceCell;
use tokio_stream::StreamExt;

#[cfg(feature = "batch")]
use super::body::read_text;
use super::body::{read_capped, ERROR_BODY_CAP};
use super::ByteStream;
use crate::provider::{ApiError, Error, ProviderOptions};

/// The `reqwest` client every provider sends through. `connect_timeout`
/// bounds the TCP/TLS handshake; `read_timeout` is the idle gap: it bounds the
/// wait for the response headers and then each body read, resetting after
/// every successful read. No redirects are followed, so a credential never
/// travels to a second origin.
fn build_client(options: &ProviderOptions) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(options.connect_timeout)
        .read_timeout(options.idle_gap)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// The [`Error`] for a `reqwest` failure: a timeout after the connection
/// stood is the idle gap running out, [`Error::IdleTimeout`]; everything
/// else, a connect timeout included, is [`Error::Http`].
pub(super) fn from_reqwest(error: reqwest::Error, idle_gap: Duration) -> Error {
    if error.is_timeout() && !error.is_connect() {
        Error::IdleTimeout { after: idle_gap }
    } else {
        Error::Http(error)
    }
}

/// The HTTP connection pool of one provider client, with its timeouts.
/// Clones share one `reqwest::Client`, built on first use unless it was
/// passed in.
#[derive(Clone, Debug)]
pub(crate) struct Http {
    options: ProviderOptions,
    client: Arc<OnceCell<reqwest::Client>>,
}

impl From<reqwest::Client> for Http {
    /// A pool around a client the caller configured; its timeouts are the
    /// caller's, and the default idle gap only labels [`Error::IdleTimeout`].
    fn from(client: reqwest::Client) -> Self {
        Self {
            options: ProviderOptions::default(),
            client: Arc::new(OnceCell::from(client)),
        }
    }
}

impl Http {
    /// A pool that builds its client from `options` on first use.
    pub(crate) fn new(options: ProviderOptions) -> Self {
        Self {
            options,
            client: Arc::new(OnceCell::new()),
        }
    }

    /// The `reqwest` client, built from the options on first use.
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
    /// its status, headers and at most 64 KiB of its body.
    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        let response = request
            .send()
            .await
            .map_err(|error| from_reqwest(error, self.idle_gap()))?;
        if response.status().is_success() {
            return Ok(response);
        }

        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_capped(response, ERROR_BODY_CAP, self.idle_gap()).await?;
        Err(Error::Api(ApiError::from_response(status, &headers, &body)))
    }

    /// Sends `request` and returns its body as byte chunks; the first error
    /// ends the stream, a read timeout as [`Error::IdleTimeout`].
    pub(crate) async fn byte_stream(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<ByteStream, Error> {
        let gap = self.idle_gap();
        let mut chunks = self.send(request).await?.bytes_stream();

        Ok(Box::pin(async_stream::try_stream! {
            while let Some(chunk) = chunks.next().await {
                yield chunk.map_err(|error| from_reqwest(error, gap))?;
            }
        }))
    }

    #[cfg(feature = "batch")]
    /// Sends `request` and returns its whole body as text.
    pub(crate) async fn text(&self, request: reqwest::RequestBuilder) -> Result<String, Error> {
        let response = self.send(request).await?;
        read_text(response, self.idle_gap()).await
    }

    #[cfg(feature = "batch")]
    /// Sends `request` and parses its whole body as `T`.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        Ok(serde_json::from_str(&self.text(request).await?)?)
    }
}

#[cfg(test)]
mod tests;
