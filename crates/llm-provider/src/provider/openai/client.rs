//! [`OpenAi`]: an endpoint speaking the OpenAI Chat Completions and
//! embeddings APIs, which hands out one [`Llm`] or [`Embedder`] per model.

use std::fmt;
use std::sync::Arc;

use crate::provider::auth::AuthMethod;
use crate::provider::channel::Channel;
use crate::provider::embedder::Embedder;
use crate::provider::llm::Llm;
use crate::provider::openai::api::OpenAiChatApi;
use crate::provider::openai::builder::OpenAiBuilder;
use crate::provider::retry::RetryingProvider;

/// The address of the OpenAI API.
const OPENAI_API: &str = "https://api.openai.com/v1";

/// An endpoint speaking the OpenAI Chat Completions API, retrying transient
/// failures: the OpenAI API, any compatible server, or Bedrock's route from
/// [`Bedrock::openai`](crate::Bedrock::openai). Clones share one HTTP
/// connection pool, and so does every [`Llm`] and [`Embedder`] it hands out.
///
/// Message batches are Anthropic's alone; an `OpenAi` has none:
///
/// ```compile_fail
/// use llm_provider::OpenAi;
///
/// let k = "k";
/// let batches = OpenAi::new(k).batches("x");
/// ```
#[derive(Clone)]
pub struct OpenAi {
    chat: Arc<RetryingProvider<OpenAiChatApi>>,
}

impl OpenAi {
    /// A client for the OpenAI API with `key`.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let openai = OpenAi::new("sk-…");
    /// # let _ = openai;
    /// ```
    #[must_use]
    pub fn new(key: impl Into<AuthMethod>) -> Self {
        Self::builder(OPENAI_API).credential(key).build()
    }

    /// A client for the server at `url` with the defaults and no credential.
    /// `url` is the whole API root, such as `http://localhost:11434/v1`; a
    /// trailing `/` is ignored.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let ollama = OpenAi::at("http://localhost:11434/v1");
    /// # let _ = ollama;
    /// ```
    #[must_use]
    pub fn at(url: impl Into<String>) -> Self {
        Self::builder(url).build()
    }

    /// A builder for a client at `url` with a credential, timeouts or a
    /// retry policy.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let openai = OpenAi::builder("https://api.openai.com/v1")
    ///     .credential("sk-…")
    ///     .build();
    /// # let _ = openai;
    /// ```
    pub fn builder(url: impl Into<String>) -> OpenAiBuilder {
        OpenAiBuilder::new(&url.into())
    }

    /// The model `model` on this client.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let qwen = OpenAi::at("http://localhost:11434/v1").llm("qwen3-coder:30b");
    /// # let _ = qwen;
    /// ```
    #[must_use]
    pub fn llm(&self, model: impl Into<String>) -> Llm {
        Llm::new(self.chat.clone(), model)
    }

    /// The embedding model `model` on this client, asking `/embeddings`.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let bge = OpenAi::at("http://localhost:11434/v1").embedder("bge-m3");
    /// # let _ = bge;
    /// ```
    #[must_use]
    pub fn embedder(&self, model: impl Into<String>) -> Embedder {
        Embedder::new(self.chat.clone(), model)
    }

    /// The endpoint on `channel` under `route`: `""` for a server named by
    /// its whole API root, `/openai/v1` on Bedrock.
    pub(crate) fn on(channel: &Arc<Channel>, route: &'static str) -> Self {
        let chat = channel.retrying(OpenAiChatApi::new(Arc::clone(channel), route));

        Self {
            chat: Arc::new(chat),
        }
    }
}

/// Hand-written because `RetryingProvider` hides the api.
impl fmt::Debug for OpenAi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAi")
            .field("base_url", &self.chat.inner().base_url())
            .finish()
    }
}

#[cfg(test)]
mod tests;
