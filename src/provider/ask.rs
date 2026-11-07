//! One-question requests: [`Request`] turns a prompt and a few options into
//! a [`CompletionRequest`] and sends it through any [`LlmProvider`].
//!
//! ```no_run
//! use llm_provider::{Anthropic, LlmProvider};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…", "claude-sonnet-4-6");
//! let provider: &dyn LlmProvider = &claude;
//! let answer = provider.request("Name three primes.").system("Be brief.").send().await?;
//! # let _ = answer; Ok(()) }
//! ```

use std::sync::Arc;

use crate::provider::error::Error;
use crate::provider::traits::{
    Completion, CompletionRequest, LlmProvider, Message, ModelSelection,
};

/// The output token limit of a [`Request`] unless [`max_tokens`](Request::max_tokens) sets one.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// A single-prompt request under construction, started by
/// [`request`](trait.LlmProvider.html#method.request) on a provider.
#[must_use]
pub struct Request<'a> {
    provider: &'a dyn LlmProvider,
    prompt: String,
    system: Option<String>,
    model: Option<String>,
    max_tokens: u32,
}

impl dyn LlmProvider + '_ {
    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] the provider returns.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        Ok(self.request(prompt).send().await?.text)
    }

    /// Starts a request with an optional system prompt, model and token
    /// limit (default 4096).
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Request {
            provider: self,
            prompt: prompt.into(),
            system: None,
            model: None,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }
}

impl Request<'_> {
    /// Sets the system prompt.
    pub fn system(mut self, text: impl Into<String>) -> Self {
        self.system = Some(text.into());
        self
    }

    /// Overrides the provider's model for this request.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the output token limit.
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    /// The [`CompletionRequest`] this builder stands for: one user message,
    /// no tools, no cache breakpoints.
    #[must_use]
    pub fn into_request(self) -> CompletionRequest {
        CompletionRequest {
            model: self
                .model
                .map_or(ModelSelection::Default, ModelSelection::Named),
            system: self.system,
            messages: Arc::from(vec![Message::user(self.prompt)]),
            tools: Vec::new(),
            max_tokens: self.max_tokens,
            cache_breakpoints: Vec::new(),
        }
    }

    /// Sends the request and waits for the full completion.
    ///
    /// # Errors
    /// Any [`Error`] the provider returns.
    pub async fn send(self) -> Result<Completion, Error> {
        let provider = self.provider;
        provider.complete(self.into_request()).await
    }
}

#[cfg(test)]
mod tests;
