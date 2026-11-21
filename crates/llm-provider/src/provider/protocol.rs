//! [`Protocol`]: the wire protocols of the models, apart from any one model,
//! and [`Provided`], a caller's own provider as one.

use serde_json::Value;

use crate::provider::error::Error;
use crate::provider::{CompletionRequest, EventStream, LlmProvider};

/// One way of sending a [`CompletionRequest`], without a model of its own:
/// the caller names the model for each request.
pub(crate) trait Protocol: Send + Sync {
    /// Streams `request`; `model` is used for `ModelSelection::Default`. The
    /// default is [`encode`](Self::encode) followed by
    /// [`stream_body`](Self::stream_body); an encoding error is the stream's
    /// only item.
    fn stream(&self, model: &str, request: CompletionRequest) -> EventStream {
        match self.encode(model, &request) {
            Ok(body) => self.stream_body(body),
            Err(error) => failed(error),
        }
    }

    /// The streaming body of `request`, before it is serialized. Pure: no
    /// I/O. The default is for a protocol without a body.
    fn encode(&self, _model: &str, _request: &CompletionRequest) -> Result<Value, Error> {
        Err(Error::Unsupported("this protocol has no request body"))
    }

    /// Serializes `body`, sends it with the endpoint's auth and retry, and
    /// decodes the reply. The default is for a protocol without a body.
    fn stream_body(&self, _body: Value) -> EventStream {
        failed(Error::Unsupported("this protocol has no request body"))
    }

    /// The protocol's name, as `ProviderInfo::name` reports it.
    fn name(&self) -> &str;
}

/// A stream whose only item is `error`.
pub(crate) fn failed(error: Error) -> EventStream {
    Box::pin(tokio_stream::once(Err(error)))
}

/// A caller's own provider as a protocol: it resolves
/// `ModelSelection::Default` itself, so the request passes through as is.
pub(crate) struct Provided<P> {
    pub(crate) provider: P,
    pub(crate) name: String,
}

// The calls are fully qualified: one `P` may implement both provider traits.
impl<P: LlmProvider> Protocol for Provided<P> {
    fn stream(&self, _model: &str, request: CompletionRequest) -> EventStream {
        LlmProvider::stream(&self.provider, request)
    }

    fn name(&self) -> &str {
        &self.name
    }
}
