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
#[cfg(test)]
use crate::transport::Http;

/// A chat model on a protocol client. Clones, and every `Llm` from one
/// client, share that client's connection pool and retry policy.
#[derive(Clone)]
pub struct Llm {
    protocol: Arc<dyn ChatProtocol>,
    model: String,
}

impl Llm {
    /// The role for `model` on `protocol`.
    pub(crate) fn new(protocol: Arc<dyn ChatProtocol>, model: impl Into<String>) -> Self {
        Self {
            protocol,
            model: model.into(),
        }
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

    /// The connection pool of the protocol client, if it has one.
    #[cfg(test)]
    pub(crate) fn http(&self) -> Option<&Http> {
        self.protocol.http()
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
