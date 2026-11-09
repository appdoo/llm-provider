//! [`OpenAiChatApi`]: one attempt over the wire to `/chat/completions`.

use std::sync::Arc;

use secrecy::ExposeSecret;
use tokio_stream::StreamExt;

use crate::provider::auth::AuthMethod;
use crate::provider::channel::Channel;
use crate::provider::error::Error;
use crate::provider::openai::request::ChatBody;
use crate::provider::openai::stream::decode;
use crate::provider::request::CompletionRequest;
use crate::provider::ChatProtocol;
use crate::provider::EventStream;
use crate::transport::ByteStream;

/// The path of the Chat Completions endpoint below the API root.
const CHAT_PATH: &str = "/chat/completions";

/// One attempt against a Chat Completions server at an [`Channel`].
/// [`OpenAi`](super::OpenAi) wraps it in retry; clones share the channel.
#[derive(Clone, Debug)]
pub(crate) struct OpenAiChatApi {
    channel: Arc<Channel>,
}

impl OpenAiChatApi {
    /// The API at `channel`.
    pub(super) fn new(channel: Arc<Channel>) -> Self {
        Self { channel }
    }

    /// The API root requests go to.
    pub(super) fn base_url(&self) -> &str {
        self.channel.origin()
    }

    /// A `POST` to `path` below the API root, with `Authorization: Bearer`
    /// when a credential is set. The one place the secret is read.
    async fn post(&self, path: &str) -> Result<reqwest::RequestBuilder, Error> {
        let url = format!("{}{path}", self.base_url());
        let post = self.channel.http().client().await?.post(url);

        Ok(match self.channel.credential() {
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

        self.channel.http().byte_stream(post).await
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
}

#[cfg(test)]
mod tests;
