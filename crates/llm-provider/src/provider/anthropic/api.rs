//! [`AnthropicApi`]: one attempt over the wire to the Messages API.

use reqwest::header::HeaderName;
use reqwest::{Method, RequestBuilder};
use secrecy::ExposeSecret;

use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::stream::decode;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{decoded, Route};
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

/// One attempt against the Messages API on a [`Route`].
/// [`Anthropic`](super::Anthropic) wraps it in retry; clones share the
/// channel.
#[derive(Clone, Debug)]
pub(crate) struct AnthropicApi {
    route: Route,
}

impl AnthropicApi {
    /// The API on `route`: the origin itself on the Anthropic API,
    /// `/anthropic` on Bedrock.
    pub(super) fn new(route: Route) -> Self {
        Self { route }
    }

    /// The route requests go to.
    pub(super) fn route(&self) -> &Route {
        &self.route
    }

    /// The origin and route requests go to, without a trailing `/`:
    /// `https://api.anthropic.com`, or on Bedrock
    /// `https://bedrock-runtime.{region}.amazonaws.com/anthropic`.
    pub(super) fn base_url(&self) -> String {
        self.route.base_url()
    }

    /// A request to `path` below the route carrying `anthropic-version` and
    /// the credential: `x-api-key` for an API key; `Authorization: Bearer`
    /// plus `anthropic-beta: oauth-2025-04-20` for an OAuth token, without
    /// the beta on Bedrock. An invalid Bedrock region fails here, before any
    /// I/O. The one place Anthropic headers are set.
    pub(super) async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, Error> {
        let request = self
            .route
            .request(method, path)
            .await?
            .header(ANTHROPIC_VERSION, API_VERSION);
        let channel = self.route.channel();

        Ok(match channel.credential() {
            Some(AuthMethod::ApiKey(key)) => request.header(X_API_KEY, key.expose_secret()),
            Some(AuthMethod::Bearer(token)) if channel.is_bedrock() => {
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
        self.route
            .channel()
            .http()
            .byte_stream(post.json(&body))
            .await
    }
}

impl ChatProtocol for AnthropicApi {
    /// Renders the streaming body (an invalid breakpoint layout fails before
    /// any I/O), posts it to [`MESSAGES_PATH`] and decodes the event stream.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        let (api, model) = (self.clone(), model.to_owned());
        decoded(
            async move { api.open_stream(&model, &request).await },
            decode,
        )
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

#[cfg(test)]
mod tests;
