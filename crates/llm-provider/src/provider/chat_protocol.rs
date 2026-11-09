//! [`ChatProtocol`]: the wire protocols of the chat models, apart from any
//! one model.

use crate::provider::{CompletionRequest, EventStream};
#[cfg(test)]
use crate::transport::Http;

/// One way of speaking to a chat model, without a model of its own: the
/// caller names the model for each request.
pub(crate) trait ChatProtocol: Send + Sync {
    /// Streams `request`; `model` is used for `ModelSelection::Default`.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream;

    /// The protocol's name, as `ProviderInfo::name` reports it.
    fn name(&self) -> &str;

    /// The connection pool this protocol sends through, if it has one.
    #[cfg(test)]
    fn http(&self) -> Option<&Http> {
        None
    }
}
