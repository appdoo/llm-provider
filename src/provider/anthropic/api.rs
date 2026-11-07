//! [`AnthropicApi`]: one attempt over the wire to the Messages API.

use async_trait::async_trait;
use reqwest::header::HeaderName;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio_stream::StreamExt;

use crate::provider::anthropic::builder::AnthropicBuilder;
use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::stream::decode;
use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::traits::{CompletionRequest, LlmProvider, ProviderInfo};
use crate::provider::EventStream;
use crate::transport::{ByteStream, Http};

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

/// One attempt against the Messages API: the connection pool and the
/// settings it was built from. [`Anthropic`](super::Anthropic) wraps it in
/// retry; clones share the pool.
#[derive(Clone, Debug)]
pub(crate) struct AnthropicApi {
    settings: AnthropicBuilder,
    http: Http,
}

impl AnthropicApi {
    /// The API for `settings`, sending through `http`.
    pub(super) fn new(settings: AnthropicBuilder, http: Http) -> Self {
        Self { settings, http }
    }

    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default) uses.
    pub(crate) fn model(&self) -> &str {
        &self.settings.model
    }

    /// The API origin without a trailing `/`.
    pub(crate) fn base_url(&self) -> &str {
        &self.settings.base_url
    }

    /// Sends `method` to `path`, with `body` as JSON when given, and parses
    /// the answer as `T`.
    pub(crate) async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + Sync)>,
    ) -> Result<T, Error> {
        let mut request = self.request(method, path).await?;
        if let Some(body) = body {
            request = request.json(body);
        }

        self.http.json(request).await
    }

    /// Sends a GET to `path` and returns the answer as text.
    pub(crate) async fn get_text(&self, path: &str) -> Result<String, Error> {
        let request = self.request(Method::GET, path).await?;
        self.http.text(request).await
    }

    /// A request to `path` on the origin carrying `anthropic-version` and the
    /// credential: `x-api-key` for an API key; `Authorization: Bearer` plus
    /// `anthropic-beta: oauth-2025-04-20` for an OAuth token. The one place
    /// Anthropic headers are set.
    async fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder, Error> {
        let url = format!("{}{path}", self.base_url());
        let request = self
            .http
            .client()
            .await?
            .request(method, url)
            .header(ANTHROPIC_VERSION, API_VERSION);

        Ok(match &self.settings.credential {
            AuthMethod::ApiKey(key) => request.header(X_API_KEY, key.expose_secret()),
            AuthMethod::Bearer(token) => request
                .bearer_auth(token.expose_secret())
                .header(ANTHROPIC_BETA, OAUTH_BETA),
        })
    }

    /// Renders `request` as a streaming body and sends it to
    /// [`MESSAGES_PATH`]; an invalid breakpoint layout fails before any I/O.
    async fn open_stream(&self, request: &CompletionRequest) -> Result<ByteStream, Error> {
        let body = MessagesBody::new(request, self.model())?.streaming();
        let post = self.request(Method::POST, MESSAGES_PATH).await?;
        self.http.byte_stream(post.json(&body)).await
    }
}

#[async_trait]
impl LlmProvider for AnthropicApi {
    /// Renders the streaming body (an invalid breakpoint layout fails before
    /// any I/O), posts it to [`MESSAGES_PATH`] and decodes the event stream.
    fn stream(&self, request: CompletionRequest) -> EventStream {
        let api = self.clone();
        Box::pin(async_stream::try_stream! {
            let mut events = decode(api.open_stream(&request).await?);
            while let Some(event) = events.next().await {
                yield event?;
            }
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "anthropic".to_string(),
            model: self.model().to_string(),
            supports_tools: true,
            supports_streaming: true,
        }
    }
}

#[cfg(test)]
mod tests;
