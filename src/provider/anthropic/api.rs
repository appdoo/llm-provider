//! [`AnthropicApi`]: one attempt over the wire to the Messages API.

use async_trait::async_trait;
use reqwest::header::HeaderName;
use secrecy::ExposeSecret;

use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::traits::{CompletionRequest, LlmProvider, ProviderInfo};
use crate::provider::EventStream;
use crate::transport::Http;

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

/// One attempt against the Messages API: the connection pool, the
/// credential, the default model and the origin.
/// [`Anthropic`](super::Anthropic) wraps it in retry; clones share the pool.
#[derive(Clone, Debug)]
pub(crate) struct AnthropicApi {
    http: Http,
    credential: AuthMethod,
    model: String,
    base_url: String,
}

impl AnthropicApi {
    /// A transport for `model` at `base_url`; a trailing `/` is dropped.
    pub(crate) fn new(http: Http, credential: AuthMethod, model: String, base_url: &str) -> Self {
        Self {
            http,
            credential,
            model,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default) uses.
    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    /// The API origin without a trailing `/`.
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The connection pool requests are sent through.
    pub(crate) fn http(&self) -> &Http {
        &self.http
    }

    /// A request to `path` on the origin carrying `anthropic-version` and the
    /// credential: `x-api-key` for an API key; `Authorization: Bearer` plus
    /// `anthropic-beta: oauth-2025-04-20` for an OAuth token. The one place
    /// Anthropic headers are set.
    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, Error> {
        let url = format!("{}{path}", self.base_url);
        let request = self
            .http
            .client()
            .await?
            .request(method, url)
            .header(ANTHROPIC_VERSION, API_VERSION);
        Ok(match &self.credential {
            AuthMethod::ApiKey(key) => request.header(X_API_KEY, key.expose_secret()),
            AuthMethod::Bearer(token) => request
                .bearer_auth(token.expose_secret())
                .header(ANTHROPIC_BETA, OAUTH_BETA),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicApi {
    /// Renders the streaming body (an invalid breakpoint layout fails before
    /// any I/O), posts it to [`MESSAGES_PATH`] and decodes the event stream.
    fn stream(&self, request: CompletionRequest) -> EventStream {
        todo!()
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "anthropic".to_string(),
            model: self.model.clone(),
            supports_tools: true,
            supports_streaming: true,
        }
    }
}
