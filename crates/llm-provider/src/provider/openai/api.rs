//! [`OpenAiChatApi`]: one attempt over the wire to `/chat/completions`.

use reqwest::{Method, RequestBuilder};
use secrecy::ExposeSecret;

use crate::provider::auth::AuthMethod;
use crate::provider::channel::{decoded, Channel, Route};
use crate::provider::error::Error;
use crate::provider::openai::request::ChatBody;
use crate::provider::openai::stream::decode;
use crate::provider::request::CompletionRequest;
use crate::provider::ChatProtocol;
use crate::provider::EventStream;
use crate::transport::ByteStream;

/// The path of the Chat Completions endpoint below the API root.
const CHAT_PATH: &str = "/chat/completions";

/// One attempt against a Chat Completions server on a [`Route`].
/// [`OpenAi`](super::OpenAi) wraps it in retry; clones share the channel.
#[derive(Clone, Debug)]
pub(crate) struct OpenAiChatApi {
    route: Route,
}

impl OpenAiChatApi {
    /// The API on `route`: the origin itself for a server named by its whole
    /// API root, `/openai/v1` on Bedrock.
    pub(super) fn new(route: Route) -> Self {
        Self { route }
    }

    /// The API root requests go to: the origin and the route.
    pub(super) fn base_url(&self) -> String {
        self.route.base_url()
    }

    /// The channel requests go through.
    pub(super) fn channel(&self) -> &Channel {
        self.route.channel()
    }

    /// A `POST` to `path` below the API root, with `Authorization: Bearer`
    /// when a credential is set. An invalid Bedrock region fails here,
    /// before any I/O. The one place the secret is read.
    pub(super) async fn post(&self, path: &str) -> Result<RequestBuilder, Error> {
        let post = self.route.request(Method::POST, path).await?;

        Ok(match self.channel().credential() {
            Some(AuthMethod::ApiKey(key) | AuthMethod::Bearer(key)) => {
                post.bearer_auth(key.expose_secret())
            }
            None => post,
        })
    }

    /// Renders `request` for `model` as a streaming body, before any I/O, and
    /// posts it to [`CHAT_PATH`].
    async fn open_stream(
        &self,
        model: &str,
        request: &CompletionRequest,
    ) -> Result<ByteStream, Error> {
        let body = ChatBody::new(request, model)?;
        let post = self.post(CHAT_PATH).await?.json(&body);

        self.channel().http().byte_stream(post).await
    }
}

impl ChatProtocol for OpenAiChatApi {
    /// Renders the streaming body, posts it to [`CHAT_PATH`] and decodes the
    /// event stream.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        let (api, model) = (self.clone(), model.to_owned());
        decoded(
            async move { api.open_stream(&model, &request).await },
            decode,
        )
    }

    fn name(&self) -> &'static str {
        "openai"
    }
}

#[cfg(test)]
mod tests;
