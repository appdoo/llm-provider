//! [`OpenAiCompatible`]: the public client, which sends every request
//! through its retry policy to a Chat Completions server.

use async_trait::async_trait;

use crate::provider::ask::Request;
use crate::provider::error::Error;
use crate::provider::openai::api::OpenAiChatApi;
use crate::provider::openai::builder::OpenAiBuilder;
use crate::provider::request::CompletionRequest;
use crate::provider::retry::RetryingProvider;
use crate::provider::EventStream;
use crate::provider::{LlmProvider, ProviderInfo};

/// A client for an OpenAI-compatible Chat Completions server that streams
/// completions, retrying transient failures. Clones share one HTTP
/// connection pool.
#[derive(Clone, Debug)]
pub struct OpenAiCompatible {
    inner: RetryingProvider<OpenAiChatApi>,
}

impl OpenAiCompatible {
    /// A builder for a client with a credential, timeouts or a retry policy.
    /// `base_url` is the API root, such as `http://localhost:11434/v1`.
    ///
    /// ```no_run
    /// use llm_provider::OpenAiCompatible;
    ///
    /// let openai = OpenAiCompatible::builder("https://api.openai.com/v1", "gpt-4o-mini")
    ///     .credential("sk-…")
    ///     .build();
    /// # let _ = openai;
    /// ```
    pub fn builder(base_url: impl Into<String>, model: impl Into<String>) -> OpenAiBuilder {
        OpenAiBuilder::new(&base_url.into(), model.into())
    }

    /// A client for `model` at `base_url` (the API root, such as
    /// `http://localhost:11434/v1`) with the defaults and no credential. It
    /// takes the base URL because a local server has no default address.
    ///
    /// ```no_run
    /// use llm_provider::OpenAiCompatible;
    ///
    /// let ollama = OpenAiCompatible::new("http://localhost:11434/v1", "llama3.2");
    /// # let _ = ollama;
    /// ```
    #[must_use]
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self::builder(base_url, model).build()
    }

    /// Wraps one-attempt calls in their retry policy; only
    /// [`OpenAiBuilder::build`] calls it.
    pub(crate) fn from_retrying(inner: RetryingProvider<OpenAiChatApi>) -> Self {
        Self { inner }
    }

    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] of the request.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        (self as &dyn LlmProvider).ask(prompt).await
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        (self as &dyn LlmProvider).request(prompt)
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        self.inner.stream(request)
    }

    fn info(&self) -> ProviderInfo {
        self.inner.info()
    }
}

#[cfg(test)]
mod tests;
