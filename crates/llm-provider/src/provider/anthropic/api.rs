//! [`AnthropicApi`]: one attempt over the wire to the Messages API.

use std::sync::Arc;

use reqwest::header::HeaderName;
use reqwest::Method;
use secrecy::ExposeSecret;
#[cfg(feature = "batch")]
use serde::de::DeserializeOwned;
#[cfg(feature = "batch")]
use serde::Serialize;
use tokio_stream::StreamExt;

use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::stream::decode;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::Channel;
use crate::provider::error::Error;
use crate::provider::EventStream;
use crate::provider::{ChatProtocol, CompletionRequest};
use crate::transport::ByteStream;

/// The header naming the API version every request is written against.
const ANTHROPIC_VERSION: HeaderName = HeaderName::from_static("anthropic-version");
/// The header that switches on beta features, here OAuth.
const ANTHROPIC_BETA: HeaderName = HeaderName::from_static("anthropic-beta");
/// The header carrying an API key.
const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");
/// The API version every request carries.
const API_VERSION: &str = "2023-06-01";
/// The beta an OAuth token needs.
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// The path of the Messages endpoint.
const MESSAGES_PATH: &str = "/v1/messages";

/// One attempt against the Messages API on a [`Channel`], under a route.
/// [`Anthropic`](super::Anthropic) wraps it in retry; clones share the
/// channel.
#[derive(Clone, Debug)]
pub(crate) struct AnthropicApi {
    channel: Arc<Channel>,
    route: &'static str,
}

impl AnthropicApi {
    /// The API on `channel` under `route`: `""` on the Anthropic API,
    /// `/anthropic` on Bedrock.
    pub(super) fn new(channel: Arc<Channel>, route: &'static str) -> Self {
        Self { channel, route }
    }

    /// The channel requests go through.
    #[cfg(feature = "batch")]
    pub(crate) fn channel(&self) -> &Arc<Channel> {
        &self.channel
    }

    /// The origin and route requests go to, without a trailing `/`:
    /// `https://api.anthropic.com`, or on Bedrock
    /// `https://bedrock-runtime.{region}.amazonaws.com/anthropic`.
    pub(crate) fn base_url(&self) -> String {
        format!("{}{}", self.channel.origin(), self.route)
    }

    /// Message batches, the only callers of [`call`](Self::call) and
    /// [`get_text`](Self::get_text), are not available on Bedrock:
    /// [`Error::Unsupported`] there, before any request.
    #[cfg(feature = "batch")]
    fn check_batches(&self) -> Result<(), Error> {
        if self.channel.is_bedrock() {
            return Err(Error::Unsupported(
                "message batches are not available on Bedrock",
            ));
        }
        Ok(())
    }

    #[cfg(feature = "batch")]
    /// Sends `method` to `path`, with `body` as JSON when given, and parses
    /// the answer as `T`.
    pub(crate) async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + Sync)>,
    ) -> Result<T, Error> {
        self.check_batches()?;
        let mut request = self.request(method, path).await?;
        if let Some(body) = body {
            request = request.json(body);
        }

        self.channel.http().json(request).await
    }

    #[cfg(feature = "batch")]
    /// Sends a GET to `path` and returns the answer as text.
    pub(crate) async fn get_text(&self, path: &str) -> Result<String, Error> {
        self.check_batches()?;
        let request = self.request(Method::GET, path).await?;
        self.channel.http().text(request).await
    }

    /// A request to `path` on the origin carrying `anthropic-version` and the
    /// credential: `x-api-key` for an API key; `Authorization: Bearer` plus
    /// `anthropic-beta: oauth-2025-04-20` for an OAuth token, without the
    /// beta on Bedrock. An invalid Bedrock region fails here, before any I/O.
    /// The one place Anthropic headers are set.
    async fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder, Error> {
        self.channel.check_region()?;
        let url = format!("{}{path}", self.base_url());
        let request = self
            .channel
            .http()
            .client()
            .await?
            .request(method, url)
            .header(ANTHROPIC_VERSION, API_VERSION);

        Ok(match self.channel.credential() {
            Some(AuthMethod::ApiKey(key)) => request.header(X_API_KEY, key.expose_secret()),
            Some(AuthMethod::Bearer(token)) if self.channel.is_bedrock() => {
                request.bearer_auth(token.expose_secret())
            }
            Some(AuthMethod::Bearer(token)) => request
                .bearer_auth(token.expose_secret())
                .header(ANTHROPIC_BETA, OAUTH_BETA),
            None => request,
        })
    }

    /// Renders `request` for `model` as a streaming body and sends it to
    /// [`MESSAGES_PATH`]; an invalid breakpoint layout fails before any I/O.
    async fn open_stream(
        &self,
        model: &str,
        request: &CompletionRequest,
    ) -> Result<ByteStream, Error> {
        let body = MessagesBody::new(request, model)?.streaming();
        let post = self.request(Method::POST, MESSAGES_PATH).await?;
        self.channel.http().byte_stream(post.json(&body)).await
    }
}

impl ChatProtocol for AnthropicApi {
    /// Renders the streaming body (an invalid breakpoint layout fails before
    /// any I/O), posts it to [`MESSAGES_PATH`] and decodes the event stream.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        let api = self.clone();
        let model = model.to_owned();

        Box::pin(async_stream::try_stream! {
            let mut events = decode(api.open_stream(&model, &request).await?);
            while let Some(event) = events.next().await {
                yield event?;
            }
        })
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

#[cfg(test)]
mod tests;
