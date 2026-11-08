//! [`LlmProvider`]: the trait every provider implements, and its
//! [`ProviderInfo`].

use async_trait::async_trait;

use crate::provider::completion::{fold, Completion};
use crate::provider::error::Error;
use crate::provider::request::CompletionRequest;
use crate::provider::EventStream;

/// The name and model of a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    /// The provider's name, such as `anthropic` or `openai-compatible`.
    pub name: String,
    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default)
    /// runs on.
    pub model: String,
}

/// A model provider that streams completions.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Streams the completion of `request`, event by event; the first error
    /// ends the stream.
    fn stream(&self, request: CompletionRequest) -> EventStream;

    /// The provider's name and model.
    fn info(&self) -> ProviderInfo;

    /// Streams `request` once and folds the
    /// events into one [`Completion`]; any error aborts it.
    ///
    /// # Errors
    /// The first error of the stream, or [`Error::UnexpectedEnd`] when it
    /// ends before its last event.
    async fn complete(&self, request: CompletionRequest) -> Result<Completion, Error> {
        fold(self.stream(request), |_| {}).await
    }
}

#[cfg(test)]
mod tests;
