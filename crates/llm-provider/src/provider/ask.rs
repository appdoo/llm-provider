//! One-question requests: [`Request`] turns a prompt and a few options into
//! a [`CompletionRequest`] and sends it through any [`LlmProvider`], for an
//! answer as text, as a caller's type, or streamed as it is written.
//!
//! ```no_run
//! use llm_provider::{Anthropic, LlmProvider};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
//! let provider: &dyn LlmProvider = &claude;
//! let answer = provider.request("Name three primes.").system("Be brief.").send().await?;
//! # let _ = answer; Ok(()) }
//! ```
//!
//! A typed answer names its shape with a [`ResponseSchema`] and parses into
//! the caller's type:
//!
//! ```no_run
//! use llm_provider::{Anthropic, ResponseSchema};
//! use serde::Deserialize;
//! use serde_json::json;
//!
//! #[derive(Deserialize)]
//! struct Primes {
//!     primes: Vec<u32>,
//! }
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
//! let schema = ResponseSchema::new(
//!     "Primes",
//!     json!({
//!         "type": "object",
//!         "properties": {"primes": {"type": "array", "items": {"type": "integer"}}},
//!         "required": ["primes"]
//!     }),
//! );
//! let answer: Primes = claude.request("Name three primes.").send_as(schema).await?;
//! # let _ = answer.primes; Ok(()) }
//! ```

use std::future::Future;
use std::sync::Arc;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::provider::error::Error;
use crate::provider::{
    Completion, CompletionRequest, EventStream, LlmProvider, Message, ModelSelection,
    ResponseSchema, StopReason, TextStream, ToolDefinition,
};

/// The output token limit of a [`Request`] unless [`max_tokens`](Request::max_tokens) sets one.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// How many characters of a bad answer an [`Error::InvalidAnswer`] keeps.
const ANSWER_EXCERPT_CHARS: usize = 200;

/// A single-prompt request under construction, started by
/// [`request`](trait.LlmProvider.html#method.request) on a provider.
#[must_use]
pub struct Request<'a> {
    provider: &'a dyn LlmProvider,
    prompt: String,
    system: Option<String>,
    model: Option<String>,
    max_tokens: u32,
    tools: Vec<ToolDefinition>,
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
        Request::new(self, prompt)
    }
}

/// [`request`](Ask::request) and [`ask`](Ask::ask) on any sized
/// [`LlmProvider`], such as a generic `P: LlmProvider`; `dyn LlmProvider`
/// and the front types carry the same methods inherently.
///
/// ```no_run
/// use llm_provider::{Ask, LlmProvider};
///
/// async fn primes<P: LlmProvider>(provider: &P) -> Result<String, llm_provider::Error> {
///     provider.request("Name three primes.").system("Be brief.").send().await.map(|c| c.text)
/// }
/// ```
pub trait Ask: LlmProvider + Sized {
    /// Starts a request with an optional system prompt, model and token
    /// limit (default 4096).
    fn request(&self, prompt: impl Into<String>) -> Request<'_> {
        Request::new(self, prompt)
    }

    /// Asks one question and returns the answer text.
    ///
    /// # Errors
    /// Any [`Error`] the provider returns.
    fn ask(
        &self,
        prompt: impl Into<String>,
    ) -> impl Future<Output = Result<String, Error>> + Send + '_ {
        let request = Ask::request(self, prompt);
        async move { Ok(request.send().await?.text) }
    }
}

impl<P: LlmProvider> Ask for P {}

impl<'a> Request<'a> {
    /// A request for `prompt` with no system prompt, the provider's model
    /// and the default token limit.
    fn new(provider: &'a dyn LlmProvider, prompt: impl Into<String>) -> Self {
        Request {
            provider,
            prompt: prompt.into(),
            system: None,
            model: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            tools: Vec::new(),
        }
    }

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

    /// Adds tools the model may call; a further call adds more. Running a
    /// tool and answering its call stay the caller's.
    pub fn tools(mut self, tools: impl IntoIterator<Item = ToolDefinition>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// The [`CompletionRequest`] this builder stands for: one user message,
    /// the [`tools`](Self::tools) added to it, no cache breakpoints.
    #[must_use]
    pub fn into_request(self) -> CompletionRequest {
        CompletionRequest {
            model: self
                .model
                .map_or(ModelSelection::Default, ModelSelection::Named),
            system: self.system,
            messages: Arc::from(vec![Message::user(self.prompt)]),
            tools: self.tools,
            max_tokens: self.max_tokens,
            cache_breakpoints: Vec::new(),
            response_schema: None,
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

    /// Streams the answer's text, delta by delta; see [`TextStream`].
    ///
    /// The request starts when the stream is first polled. It goes through
    /// [`LlmProvider::stream`], so an error before the first content event
    /// is retried as the provider's
    /// [`RetryPolicy`](crate::provider::RetryPolicy) says.
    pub fn stream(self) -> TextStream {
        TextStream::new(self.events())
    }

    /// Streams every event of the answer: text, tool calls, token counts
    /// and the stop reason, exactly as [`LlmProvider::stream`] yields them.
    ///
    /// ```no_run
    /// use futures::StreamExt;
    /// use llm_provider::{Anthropic, StreamEvent};
    ///
    /// # async fn run() -> Result<(), llm_provider::Error> {
    /// let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
    /// let mut events = claude.request("Name three primes.").events();
    /// while let Some(event) = events.next().await {
    ///     match event? {
    ///         StreamEvent::ContentDelta(text) => print!("{text}"),
    ///         StreamEvent::Usage(usage) => eprintln!("tokens: {}", usage.output_tokens),
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    #[must_use = "a stream does nothing until it is polled"]
    pub fn events(self) -> EventStream {
        let provider = self.provider;
        provider.stream(self.into_request())
    }

    /// Sends the request held to `schema` and parses the answer text into
    /// a `T`. Anthropic holds the text to the schema with structured
    /// outputs (`output_config.format`, type `json_schema`), which every
    /// current model accepts; an OpenAI-compatible server gets a
    /// `json_schema` response format.
    ///
    /// Structured outputs take a subset of JSON Schema, so the schema
    /// Anthropic receives has every object closed
    /// (`additionalProperties: false`), `minimum`, `maximum`, `multipleOf`,
    /// `minLength`, `maxLength`, `maxItems` and `uniqueItems` removed, and
    /// `minItems` lowered to at most 1. Checks beyond the schema, such as
    /// item counts, stay the caller's, after parsing.
    ///
    /// # Errors
    /// Any [`Error`] the provider returns; [`Error::InvalidAnswer`] when the
    /// model refused, the answer stopped at the token limit, or it is
    /// missing or does not parse into `T`.
    pub async fn send_as<T: DeserializeOwned>(self, schema: ResponseSchema) -> Result<T, Error> {
        let provider = self.provider;

        let mut request = self.into_request();
        request.response_schema = Some(schema);
        let completion = provider.complete(request).await?;

        parse_answer(&completion)
    }

    /// Sends the request held to `T`'s derived schema
    /// ([`ResponseSchema::of`]) and parses the answer into a `T`, as
    /// [`send_as`](Self::send_as) does, under the same schema subset.
    ///
    /// The type's doc comments become the descriptions the model reads.
    /// The `Deserialize` and `JsonSchema` derives expand to `serde` and
    /// `schemars` paths, so a crate that derives them depends on both
    /// itself (`serde` with `derive`, `schemars = "1"`).
    ///
    /// ```no_run
    /// use llm_provider::{Anthropic, JsonSchema};
    /// use serde::Deserialize;
    ///
    /// /// Three primes.
    /// #[derive(Deserialize, JsonSchema)]
    /// struct Primes {
    ///     /// The primes, ascending.
    ///     primes: Vec<u32>,
    /// }
    ///
    /// # async fn run() -> Result<(), llm_provider::Error> {
    /// let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
    /// let answer = claude.request("Name three primes.").send_json::<Primes>().await?;
    /// # let _ = answer.primes; Ok(()) }
    /// ```
    ///
    /// # Errors
    /// Any [`Error`] the provider returns; [`Error::InvalidAnswer`] when the
    /// answer is missing or does not parse into `T`.
    #[cfg(feature = "schema")]
    pub async fn send_json<T: DeserializeOwned + JsonSchema>(self) -> Result<T, Error> {
        self.send_as(ResponseSchema::of::<T>()).await
    }
}

/// The text of `completion` parsed into a `T`. A refusal or an answer cut
/// at the token limit is rejected before parsing, since its text need not
/// match the schema.
fn parse_answer<T: DeserializeOwned>(completion: &Completion) -> Result<T, Error> {
    let answer = &completion.text;
    match completion.stop {
        StopReason::Refusal => return Err(invalid_answer("stop_reason: refusal", answer)),
        StopReason::MaxTokens => return Err(invalid_answer("stop_reason: max_tokens", answer)),
        _ => {}
    }

    if answer.trim().is_empty() {
        return Err(invalid_answer("no answer", answer));
    }

    serde_json::from_str(answer).map_err(|error| invalid_answer(error.to_string(), answer))
}

/// An [`Error::InvalidAnswer`] for `answer`, keeping its first
/// [`ANSWER_EXCERPT_CHARS`] characters.
fn invalid_answer(reason: impl Into<String>, answer: &str) -> Error {
    Error::InvalidAnswer {
        reason: reason.into(),
        answer: answer.chars().take(ANSWER_EXCERPT_CHARS).collect(),
    }
}

#[cfg(test)]
mod tests;
