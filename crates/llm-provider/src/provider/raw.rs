//! [`RawLlm`]: one model on an OpenAI-compatible `/v1/completions` server,
//! sent a prompt the caller rendered itself.
//!
//! ```no_run
//! use llm_provider::{OpenAi, RawRequest};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let qwen = OpenAi::at("http://localhost:8080/v1")?.raw("qwen3-8b");
//! let prompt = "<|im_start|>user\nName three primes.<|im_end|>\n<|im_start|>assistant\n";
//! let completion = qwen.complete(RawRequest::new(prompt)).await?;
//! # let _ = completion; Ok(()) }
//! ```

mod request;

use std::fmt;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::provider::completion::{fold_cancellable, Completion};
use crate::provider::error::Error;
use crate::provider::openai::CompletionsBody;
use crate::provider::protocol::failed;
use crate::provider::{EventStream, Protocol};

pub use request::RawRequest;

/// A model on a raw completion endpoint: it streams a prompt the caller
/// rendered, template and prefill included, without a chat template of its
/// own. Clones, and every `RawLlm` from one endpoint, share its connection
/// pool and retry policy.
#[derive(Clone)]
pub struct RawLlm {
    protocol: Arc<dyn Protocol>,
    model: String,
}

impl RawLlm {
    /// The model `model` on `protocol`.
    pub(crate) fn new(protocol: Arc<dyn Protocol>, model: impl Into<String>) -> Self {
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

    /// Streams `request`: the answer's text as
    /// [`ContentDelta`](crate::StreamEvent::ContentDelta)s, its usage, then
    /// [`Stop`](crate::StreamEvent::Stop) and
    /// [`MessageStop`](crate::StreamEvent::MessageStop). The first error
    /// ends the stream.
    ///
    /// The caller's prompt is what the model reads; the caller may stop
    /// reading at any point, and dropping the stream aborts the request.
    ///
    /// ```no_run
    /// use llm_provider::{OpenAi, RawRequest, Sampling, StreamEvent};
    /// use tokio_stream::StreamExt;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), llm_provider::Error> {
    ///     // llama-server, local, Metal.
    ///     let model = OpenAi::at("http://localhost:8080/v1")?.raw("qwen3-8b");
    ///
    ///     let prompt = "<|im_start|>user\nWrite fib in Rust.<|im_end|>\n<|im_start|>assistant\nfn fib(";
    ///     let request = RawRequest::new(prompt)
    ///         .max_tokens(512)
    ///         .sampling(Sampling::new().temperature(0.2).top_p(0.9).seed(7).stop(["<|im_end|>"]))
    ///         .extra(serde_json::json!({"top_k": 20}).as_object().unwrap().clone());
    ///
    ///     let mut stream = model.stream(request);
    ///     let mut text = String::new();
    ///     while let Some(event) = stream.next().await {
    ///         if let StreamEvent::ContentDelta(piece) = event? {
    ///             text.push_str(&piece);
    ///             if text.contains("unsafe") {
    ///                 drop(stream); // aborts the request; the server stops generating
    ///                 break;
    ///             }
    ///         }
    ///     }
    ///     // repair `text`, append it to the prompt, send a new RawRequest: continue.
    ///     Ok(())
    /// }
    /// ```
    #[must_use = "a stream does nothing until polled"]
    #[allow(clippy::needless_pass_by_value)] // the 0.3.0 signature takes the request by value
    pub fn stream(&self, request: RawRequest) -> EventStream {
        let body = CompletionsBody::new(&request, &self.model)
            .and_then(|body| serde_json::to_value(body).map_err(Error::from));

        match body {
            Ok(body) => self.protocol.stream_body(body),
            Err(error) => failed(error),
        }
    }

    /// Streams `request` and folds the events into one [`Completion`].
    ///
    /// # Errors
    /// Any [`Error`] of the request, such as
    /// [`Error::InvalidRequest`] for a non-finite sampling value or a
    /// reserved [`extra`](RawRequest::extra) key, before any I/O.
    pub async fn complete(&self, request: RawRequest) -> Result<Completion, Error> {
        fold_cancellable(self.stream(request), |_| {}, &CancellationToken::new()).await
    }

    /// Streams `request` and folds the events into one [`Completion`],
    /// unless `token` fires first. A cancel drops the stream, which closes
    /// the request; no retry follows it.
    ///
    /// ```no_run
    /// use llm_provider::{Error, OpenAi, RawRequest};
    /// use tokio_util::sync::CancellationToken;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let model = OpenAi::at("http://localhost:8080/v1").expect("a valid URL").raw("qwen3-8b");
    ///     let token = CancellationToken::new();
    ///     let stop = token.clone();
    ///     tokio::spawn(async move { /* a new message arrives */ stop.cancel(); });
    ///
    ///     match model.complete_cancellable(RawRequest::new("…"), &token).await {
    ///         Ok(done) => println!("{}", done.text),
    ///         Err(Error::Interrupted { partial_text, .. }) => println!("cut: {partial_text}"),
    ///         Err(other) => eprintln!("{other}"),
    ///     }
    /// }
    /// ```
    ///
    /// # Errors
    /// [`Error::Interrupted`] with the text received so far when `token`
    /// fires; none is sent when it already has. Any other error of the
    /// request.
    pub async fn complete_cancellable(
        &self,
        request: RawRequest,
        token: &CancellationToken,
    ) -> Result<Completion, Error> {
        fold_cancellable(self.stream(request), |_| {}, token).await
    }
}

/// Hand-written because the protocol is a trait object.
impl fmt::Debug for RawLlm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawLlm")
            .field("protocol", &self.protocol.name())
            .field("model", &self.model)
            .finish()
    }
}
