//! [`Llm`]: one chat model on an endpoint, the type a program asks.
//!
//! ```no_run
//! use llm_provider::OpenAi;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let ollama = OpenAi::at("http://localhost:11434/v1")?;
//! let qwen = ollama.llm("qwen3-coder:30b");
//! let answer = qwen.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ModelBuilder;
use crate::provider::ask::Request;
use crate::provider::error::Error;
use crate::provider::request::CompletionRequest;
use crate::provider::{EventStream, LlmProvider, Protocol, Provided, ProviderInfo};

/// A chat model on an endpoint. Clones, and every `Llm` from one endpoint,
/// share its connection pool and retry policy.
#[derive(Clone)]
pub struct Llm {
    protocol: Arc<dyn Protocol>,
    model: String,
}

impl Llm {
    /// The model `model` on `protocol`.
    pub(crate) fn new(protocol: Arc<dyn Protocol>, model: impl Into<String>) -> Self {
        Self {
            protocol,
            model: model.into(),
        }
    }

    /// A builder for the chat model `model`, reached by values known at run
    /// time: protocol, endpoint, region and credential. Each
    /// [`build`](ModelBuilder::build) makes its own endpoint; models that
    /// share one come from an endpoint such as [`OpenAi`](crate::OpenAi).
    ///
    /// ```no_run
    /// use llm_provider::{Llm, Protocol};
    ///
    /// # fn run(key: String) -> Result<(), llm_provider::Error> {
    /// let qwen = Llm::builder("qwen3")
    ///     .protocol(Protocol::OpenAi)
    ///     .endpoint("http://gpu-box:8000/v1")
    ///     .api_key(key)
    ///     .build()?;
    /// # let _ = qwen; Ok(()) }
    /// ```
    pub fn builder(model: impl Into<String>) -> ModelBuilder<Llm> {
        ModelBuilder::new(model.into())
    }

    /// A model over a caller's own provider, such as an on-device one. Its
    /// name is the provider's `info().model`. It is not wrapped in retry.
    ///
    /// ```no_run
    /// use llm_provider::{Llm, LlmProvider};
    ///
    /// fn on_device(provider: impl LlmProvider + 'static) -> Llm {
    ///     Llm::from_provider(provider)
    /// }
    /// ```
    #[must_use]
    pub fn from_provider(provider: impl LlmProvider + 'static) -> Self {
        let info = provider.info();
        let chat = Provided {
            provider,
            name: info.name,
        };
        Self::new(Arc::new(chat), info.model)
    }

    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default)
    /// runs on.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] of the request.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        Ok(self.request(prompt).send().await?.text)
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Request::new(self, prompt)
    }
}

#[async_trait]
impl LlmProvider for Llm {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        self.protocol.stream(&self.model, request)
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: self.protocol.name().to_string(),
            model: self.model.clone(),
        }
    }
}

/// Hand-written because the protocol is a trait object.
impl fmt::Debug for Llm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Llm")
            .field("protocol", &self.protocol.name())
            .field("model", &self.model)
            .finish()
    }
}

#[cfg(test)]
mod tests;
