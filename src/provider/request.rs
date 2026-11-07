//! The one-question builder on `dyn LlmProvider`.
//!
//! The builder's `max_tokens` default of 4096 is its own; the agent loop keeps
//! `LoopDefaults::max_tokens`. Inherent methods on `dyn LlmProvider` are not
//! found from a concrete value, hence the forwarders on `Anthropic` and
//! `OpenAiCompatible`.

use std::sync::Arc;

use crate::provider::error::Error;
use crate::provider::traits::{
    Completion, CompletionRequest, LlmProvider, Message, ModelSelection,
};

/// A single-prompt request under construction; see `LlmProvider::request`.
pub struct Request<'a> {
    provider: &'a dyn LlmProvider,
    prompt: String,
    system: Option<String>,
    model: Option<String>,
    max_tokens: u32,
}

impl<'p> dyn LlmProvider + 'p {
    /// Ask one question and return the answer text.
    pub async fn ask(&self, prompt: impl Into<String>) -> Result<String, Error> {
        Ok(self.request(prompt).send().await?.text)
    }

    /// Start a request with optional system prompt, model and token limit.
    pub fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Request {
            provider: self,
            prompt: prompt.into(),
            system: None,
            model: None,
            max_tokens: 4096,
        }
    }
}

impl<'a> Request<'a> {
    /// Set the system prompt.
    pub fn system(mut self, text: impl Into<String>) -> Self {
        self.system = Some(text.into());
        self
    }

    /// Override the provider's model for this request.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the output token limit (default 4096).
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    /// The `CompletionRequest` this builder stands for.
    pub fn into_request(self) -> CompletionRequest {
        CompletionRequest {
            model: match self.model {
                Some(m) => ModelSelection::Named(m),
                None => ModelSelection::Default,
            },
            system: self.system,
            messages: Arc::from(vec![Message::user(self.prompt)]),
            tools: vec![],
            max_tokens: self.max_tokens,
            cache_breakpoints: Vec::new(),
        }
    }

    /// Send the request and wait for the full completion.
    pub async fn send(self) -> Result<Completion, Error> {
        let p = self.provider;
        p.complete(self.into_request()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::test_stub::StubServer;
    use crate::provider::Anthropic;

    async fn stub() -> StubServer {
        StubServer::start(vec![(
            ("POST", "/v1/messages"),
            (
                200,
                "text/event-stream",
                include_str!("../../tests/fixtures/anthropic_text.sse").to_string(),
            ),
        )])
        .await
    }

    /// A8: `ask` works on a boxed trait object.
    #[tokio::test]
    async fn ask_on_boxed_dyn() {
        let stub = stub().await;
        let claude = Anthropic::new("sk-test", "claude-x").base_url(stub.url());
        let p: Box<dyn LlmProvider> = Box::new(claude);
        assert_eq!(p.ask("Hello").await.expect("ask"), "Hi");
    }

    async fn via(p: &dyn LlmProvider) -> Result<String, Error> {
        p.ask("Hello").await
    }

    /// A8b: `ask` works on a borrowed `&dyn LlmProvider`.
    #[tokio::test]
    async fn ask_on_borrowed_dyn() {
        let stub = stub().await;
        let claude = Anthropic::new("sk-test", "claude-x").base_url(stub.url());
        assert_eq!(via(&claude).await.expect("ask"), "Hi");
    }
}
