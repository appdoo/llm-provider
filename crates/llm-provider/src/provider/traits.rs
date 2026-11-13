//! [`LlmProvider`]: the trait every provider implements, and its
//! [`ProviderInfo`].

use std::pin::Pin;

use async_trait::async_trait;
use tokio_stream::Stream;

use crate::provider::completion::{fold, Completion};
use crate::provider::error::Error;
use crate::provider::request::CompletionRequest;
use crate::provider::{EventStream, StreamEvent};

/// The name and model of a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderInfo {
    /// The provider's name, such as `anthropic` or `openai`.
    pub name: String,
    /// The model a request with
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default)
    /// runs on.
    pub model: String,
}

impl ProviderInfo {
    /// The provider `name` running `model` by default.
    ///
    /// ```
    /// use llm_provider::ProviderInfo;
    ///
    /// let info = ProviderInfo::new("echo", "echo-1");
    /// assert_eq!(info.model, "echo-1");
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            model: model.into(),
        }
    }
}

/// A model provider that streams completions.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Streams the completion of `request`, event by event; the first error
    /// ends the stream. The return type is an [`EventStream`], written out
    /// here so that the trait's signature names no alias.
    fn stream(
        &self,
        request: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;

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

// Hand-written forwarding: a runtime owns its provider, and a caller may
// hold one boxed (`Box<dyn LlmProvider>`) or borrowed.
#[async_trait]
impl<P: LlmProvider + ?Sized> LlmProvider for Box<P> {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        (**self).stream(request)
    }

    fn info(&self) -> ProviderInfo {
        (**self).info()
    }
}

#[async_trait]
impl<P: LlmProvider + ?Sized> LlmProvider for &P {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        (**self).stream(request)
    }

    fn info(&self) -> ProviderInfo {
        (**self).info()
    }
}

#[cfg(test)]
mod tests;
