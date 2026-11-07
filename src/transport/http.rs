//! The shared HTTP client and the one place a response's status is checked.

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::sync::OnceCell;

use crate::provider::{Error, ProviderOptions};
use crate::transport::ByteStream;

/// The `reqwest` client every provider sends through: the options' connect
/// timeout, and no redirects followed, so a credential never travels to a
/// second origin.
pub(crate) fn build_client(options: &ProviderOptions) -> Result<reqwest::Client, reqwest::Error> {
    todo!()
}

/// Sends `request`, waiting at most `idle_gap` for the response headers. A
/// non-2xx answer becomes [`Error::Api`], built from its status, headers and
/// at most [`ERROR_BODY_CAP`](crate::transport::ERROR_BODY_CAP) bytes of its
/// body.
pub(crate) async fn send_checked(
    request: reqwest::RequestBuilder,
    idle_gap: Duration,
) -> Result<reqwest::Response, Error> {
    todo!()
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
        todo!()
    }

    /// The `reqwest` client, built by [`build_client`] on first use.
    pub(crate) async fn client(&self) -> Result<&reqwest::Client, Error> {
        todo!()
    }

    /// The longest silence allowed before the headers and between body chunks.
    pub(crate) fn idle_gap(&self) -> Duration {
        todo!()
    }

    /// Sends `request` through [`send_checked`] under this pool's idle gap.
    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        todo!()
    }

    /// Sends `request` and returns its body as byte chunks, each awaited
    /// under the idle gap ([`idle_guard`](crate::transport::idle_guard)).
    pub(crate) async fn byte_stream(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<ByteStream, Error> {
        todo!()
    }

    /// Sends `request` and parses its whole body as `T`; each chunk is
    /// awaited under the idle gap.
    pub(crate) async fn json<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        todo!()
    }
}
