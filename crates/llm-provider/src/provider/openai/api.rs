//! [`OpenAiChatApi`]: one attempt over the wire to `/chat/completions`.

use async_trait::async_trait;
use secrecy::ExposeSecret;
use tokio_stream::StreamExt;

use crate::provider::auth::AuthMethod;
use crate::provider::error::Error;
use crate::provider::openai::builder::OpenAiBuilder;
use crate::provider::openai::request::ChatBody;
use crate::provider::openai::stream::decode;
use crate::provider::request::CompletionRequest;
use crate::provider::EventStream;
use crate::provider::{ChatProtocol, LlmProvider, ProviderInfo};
use crate::transport::{ByteStream, Http};

/// The path of the Chat Completions endpoint below the API root.
const CHAT_PATH: &str = "/chat/completions";

/// One attempt against a Chat Completions server: the connection pool and
/// the settings it was built from.
/// [`OpenAiCompatible`](super::OpenAiCompatible) wraps it in retry; clones
/// share the pool.
#[derive(Clone, Debug)]
pub(crate) struct OpenAiChatApi {
    settings: OpenAiBuilder,
    http: Http,
}

impl OpenAiChatApi {
    /// The API for `settings`, sending through `http`.
    pub(super) fn new(settings: OpenAiBuilder, http: Http) -> Self {
        Self { settings, http }
    }

    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default) uses.
    pub(crate) fn model(&self) -> &str {
        &self.settings.model
    }

    /// Renders `request` for `model` as a streaming body and posts it to
    /// [`CHAT_PATH`], with `Authorization: Bearer` when a credential is set.
    /// The one place the secret is read.
    async fn open_stream(
        &self,
        model: &str,
        request: &CompletionRequest,
    ) -> Result<ByteStream, Error> {
        let body = ChatBody::new(request, model)?;
        let url = format!("{}{CHAT_PATH}", self.settings.base_url);
        let mut post = self.http.client().await?.post(url).json(&body);

        if let Some(AuthMethod::ApiKey(key) | AuthMethod::Bearer(key)) = &self.settings.credential {
            post = post.bearer_auth(key.expose_secret());
        }

        self.http.byte_stream(post).await
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
        "openai-compatible"
    }
}

#[async_trait]
impl LlmProvider for OpenAiChatApi {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        ChatProtocol::stream(self, self.model(), request)
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: ChatProtocol::name(self).to_string(),
            model: self.model().to_string(),
        }
    }
}

#[cfg(test)]
mod tests;
