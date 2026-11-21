//! [`RawRequest`]: one raw prompt with its model, token limit, sampling and
//! the caller's own body keys.

use serde_json::{Map, Value};

use crate::provider::request::{ModelSelection, DEFAULT_MAX_TOKENS};
use crate::provider::Sampling;

/// One raw completion request: a prompt the caller rendered, sent as is.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RawRequest {
    /// The prompt, sent byte for byte.
    pub prompt: String,
    /// The model the request runs on.
    pub model: ModelSelection,
    /// The most tokens the answer may have.
    pub max_tokens: u32,
    /// The sampling settings; unset ones are left to the server.
    pub sampling: Sampling,
    /// Keys added to the top level of the body, such as a server's own
    /// sampling settings. A key the request sets itself (`model`, `prompt`,
    /// `max_tokens`, `stream`, `stream_options`, `temperature`, `top_p`,
    /// `seed`, `stop`) is
    /// [`RequestError::ReservedKey`](crate::provider::RequestError::ReservedKey),
    /// whether or not the matching setting is set.
    pub extra: Map<String, Value>,
}

impl RawRequest {
    /// A request for `prompt` on the default model, with 4096 output tokens,
    /// the server's sampling and no extra keys.
    ///
    /// The prompt is sent byte for byte. A server may add a BOS token and
    /// parse special-token text such as `<|im_start|>` in it into control
    /// tokens (llama-server and Ollama both do), so neutralising untrusted
    /// segments of the prompt is the caller's job.
    ///
    /// ```no_run
    /// use llm_provider::{RawRequest, Sampling};
    ///
    /// let request = RawRequest::new("<|im_start|>assistant\nfn fib(")
    ///     .max_tokens(256)
    ///     .sampling(Sampling::new().temperature(0.2));
    /// # let _ = request;
    /// ```
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            model: ModelSelection::Default,
            max_tokens: DEFAULT_MAX_TOKENS,
            sampling: Sampling::default(),
            extra: Map::new(),
        }
    }

    /// Runs the request on `model` instead of the [`RawLlm`](crate::RawLlm)'s
    /// own.
    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = ModelSelection::Named(model.into());
        self
    }

    /// Sets the most tokens the answer may have.
    #[must_use]
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the sampling settings.
    #[must_use]
    pub fn sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// Sets the keys added to the top level of the body, replacing any set
    /// before. A key the request sets itself is
    /// [`RequestError::ReservedKey`](crate::provider::RequestError::ReservedKey);
    /// see [`extra`](Self#structfield.extra) for the list.
    #[must_use]
    pub fn extra(mut self, extra: Map<String, Value>) -> Self {
        self.extra = extra;
        self
    }
}
