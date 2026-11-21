//! [`AnthropicApi`]: one attempt over the wire to the Messages API.

#[cfg(feature = "batch")]
use reqwest::{Method, RequestBuilder};
use secrecy::ExposeSecret;
use serde_json::Value;

use crate::core::Transport;
use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::stream::decode;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::Route;
use crate::provider::error::Error;
use crate::provider::EventStream;
use crate::provider::{CompletionRequest, Protocol};

/// The header naming the API version every request is written against.
const ANTHROPIC_VERSION: &str = "anthropic-version";
/// The header that switches on beta features, here OAuth.
const ANTHROPIC_BETA: &str = "anthropic-beta";
/// The header carrying an API key.
const X_API_KEY: &str = "x-api-key";
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
    transport: Transport,
}

impl AnthropicApi {
    /// The API on `route`: the origin itself on the Anthropic API,
    /// `/anthropic` on Bedrock.
    pub(super) fn new(route: Route) -> Self {
        Self {
            transport: transport(route),
        }
    }

    /// The route requests go to.
    pub(super) fn route(&self) -> &Route {
        self.transport.route()
    }

    /// The origin and route requests go to, without a trailing `/`:
    /// `https://api.anthropic.com`, or on Bedrock
    /// `https://bedrock-runtime.{region}.amazonaws.com/anthropic`.
    pub(super) fn base_url(&self) -> String {
        self.route().base_url()
    }

    /// A request to `path` below the route carrying the headers of
    /// [`transport`]. An invalid Bedrock region fails here, before any I/O.
    #[cfg(feature = "batch")]
    pub(super) async fn request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<RequestBuilder, Error> {
        self.transport.request(method, path).await
    }
}

/// The transport on `route` with `anthropic-version` and the credential:
/// `x-api-key` for an API key; `Authorization: Bearer` plus
/// `anthropic-beta: oauth-2025-04-20` for an OAuth token, without the beta
/// on Bedrock. The one place Anthropic headers are set.
fn transport(route: Route) -> Transport {
    let (credential, bedrock) = (
        route.channel().credential().cloned(),
        route.channel().is_bedrock(),
    );
    let transport = Transport::on(route).header(ANTHROPIC_VERSION, API_VERSION);

    match credential {
        Some(AuthMethod::ApiKey(key)) => transport.header(X_API_KEY, key.expose_secret()),
        Some(AuthMethod::Bearer(token)) if bedrock => transport.bearer(token.expose_secret()),
        Some(AuthMethod::Bearer(token)) => transport
            .bearer(token.expose_secret())
            .header(ANTHROPIC_BETA, OAUTH_BETA),
        None => transport,
    }
}

impl Protocol for AnthropicApi {
    /// The streaming body; an invalid breakpoint layout fails here, before
    /// any I/O.
    fn encode(&self, model: &str, request: &CompletionRequest) -> Result<Value, Error> {
        let body = MessagesBody::new(request, model)?.streaming();
        Ok(serde_json::to_value(body)?)
    }

    fn stream_body(&self, body: Value) -> EventStream {
        self.transport.send_body(MESSAGES_PATH, body, decode)
    }

    fn name(&self) -> &'static str {
        "anthropic"
    }
}

#[cfg(test)]
mod tests;
