//! [`OpenAiChatApi`]: one attempt over the wire to `/chat/completions`.

use std::sync::Arc;

use secrecy::ExposeSecret;
use tokio_stream::StreamExt;

use crate::provider::auth::AuthMethod;
use crate::provider::endpoint::Endpoint;
use crate::provider::error::Error;
use crate::provider::openai::request::ChatBody;
use crate::provider::openai::stream::decode;
use crate::provider::request::CompletionRequest;
use crate::provider::ChatProtocol;
use crate::provider::EventStream;
use crate::transport::ByteStream;
#[cfg(test)]
use crate::transport::Http;

/// The path of the Chat Completions endpoint below the API root.
const CHAT_PATH: &str = "/chat/completions";

/// One attempt against a Chat Completions server at an [`Endpoint`].
/// [`OpenAi`](super::OpenAi) wraps it in retry; clones share the endpoint.
#[derive(Clone, Debug)]
pub(crate) struct OpenAiChatApi {
    endpoint: Arc<Endpoint>,
}

impl OpenAiChatApi {
    /// The API at `endpoint`.
    pub(super) fn new(endpoint: Arc<Endpoint>) -> Self {
        Self { endpoint }
    }

    /// The API root requests go to.
    pub(super) fn base_url(&self) -> &str {
        self.endpoint.origin()
    }

    /// A `POST` to `path` below the API root, with `Authorization: Bearer`
    /// when a credential is set. The one place the secret is read.
    async fn post(&self, path: &str) -> Result<reqwest::RequestBuilder, Error> {
        let url = format!("{}{path}", self.base_url());
        let post = self.endpoint.http().client().await?.post(url);

        Ok(match self.endpoint.credential() {
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

        self.endpoint.http().byte_stream(post).await
    }
}

impl ChatProtocol for OpenAiChatApi {
    /// Renders the streaming body, posts it to [`CHAT_PATH`] and decodes the
    /// event stream.
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
        "openai"
    }

    #[cfg(test)]
    fn http(&self) -> Option<&Http> {
        Some(self.endpoint.http())
    }
}

#[cfg(test)]
mod tests;
