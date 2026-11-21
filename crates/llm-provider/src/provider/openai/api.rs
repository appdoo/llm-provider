//! [`OpenAiApi`]: one attempt over the wire to an OpenAI-compatible server.

use reqwest::{Method, RequestBuilder};
use secrecy::ExposeSecret;
use serde_json::Value;

use crate::core::{ByteStream, Transport};
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{Channel, Route};
use crate::provider::error::Error;
use crate::provider::openai::request::ChatBody;
use crate::provider::openai::stream::decode;
use crate::provider::request::CompletionRequest;
use crate::provider::EventStream;
use crate::provider::Protocol;

/// The path of the Chat Completions endpoint below the API root.
const CHAT_PATH: &str = "/chat/completions";

/// One attempt against an OpenAI-compatible server on a [`Route`].
/// [`OpenAi`](super::OpenAi) wraps it in retry; clones share the channel.
#[derive(Clone, Debug)]
pub(crate) struct OpenAiApi {
    transport: Transport,
}

impl OpenAiApi {
    /// The API on `route`: the origin itself for a server named by its whole
    /// API root, `/openai/v1` on Bedrock.
    pub(super) fn new(route: Route) -> Self {
        let credential = route.channel().credential().cloned();
        let transport = Transport::on(route);
        let transport = match credential {
            Some(AuthMethod::ApiKey(key) | AuthMethod::Bearer(key)) => {
                transport.bearer(key.expose_secret())
            }
            None => transport,
        };

        Self { transport }
    }

    /// The API root requests go to: the origin and the route.
    pub(super) fn base_url(&self) -> String {
        self.route().base_url()
    }

    /// The channel requests go through.
    pub(super) fn channel(&self) -> &Channel {
        self.route().channel()
    }

    /// A `POST` to `path` below the API root, with `Authorization: Bearer`
    /// when a credential is set. An invalid Bedrock region fails here,
    /// before any I/O.
    pub(super) async fn post(&self, path: &str) -> Result<RequestBuilder, Error> {
        self.transport.request(Method::POST, path).await
    }

    /// The channel's route: the origin, Bedrock or not.
    pub(super) fn route(&self) -> &Route {
        self.transport.route()
    }

    /// `body` posted to `path` through the transport: the one send path of
    /// the OpenAI wire formats.
    pub(super) fn send_body(
        &self,
        path: &str,
        body: Value,
        decode: fn(ByteStream) -> EventStream,
    ) -> EventStream {
        self.transport.send_body(path, body, decode)
    }
}

impl Protocol for OpenAiApi {
    /// The streaming body, rendered before any I/O.
    fn encode(&self, model: &str, request: &CompletionRequest) -> Result<Value, Error> {
        Ok(serde_json::to_value(ChatBody::new(request, model)?)?)
    }

    fn stream_body(&self, body: Value) -> EventStream {
        self.send_body(CHAT_PATH, body, decode)
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}

#[cfg(test)]
mod tests;
