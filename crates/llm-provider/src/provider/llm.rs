//! [`Llm`]: one chat model behind one protocol client, the type a program
//! asks.
//!
//! ```no_run
//! use llm_provider::OpenAi;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let ollama = OpenAi::at("http://localhost:11434/v1");
//! let qwen = ollama.llm("qwen3-coder:30b");
//! let answer = qwen.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::provider::ask::{Ask, Request};
use crate::provider::error::Error;
use crate::provider::request::CompletionRequest;
use crate::provider::{ChatProtocol, EventStream, LlmProvider, ProviderInfo};

/// A chat model on a protocol client. Clones, and every `Llm` from one
/// client, share that client's connection pool and retry policy.
#[derive(Clone)]
pub struct Llm {
    protocol: Arc<dyn ChatProtocol>,
    model: String,
}

impl Llm {
    /// The model `model` on `protocol`.
    pub(crate) fn new(protocol: Arc<dyn ChatProtocol>, model: impl Into<String>) -> Self {
        Self {
            protocol,
            model: model.into(),
        }
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
        let chat = ProviderChat {
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
        Ask::ask(self, prompt).await
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Ask::request(self, prompt)
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

/// A caller's own provider as a protocol: it resolves
/// `ModelSelection::Default` itself, so the request passes through as is.
struct ProviderChat<P> {
    provider: P,
    name: String,
}

impl<P: LlmProvider> ChatProtocol for ProviderChat<P> {
    fn stream(&self, _model: &str, request: CompletionRequest) -> EventStream {
        self.provider.stream(request)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests;
