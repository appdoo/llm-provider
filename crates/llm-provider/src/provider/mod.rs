//! The provider layer: the [`LlmProvider`] trait, the [`Anthropic`] and
//! [`OpenAi`] clients, the [`Llm`] role, the one-question [`Request`] and
//! retry.
//!
//! # Protocols
//!
//! - **Anthropic Messages API** ([`Anthropic`]): Claude, with prompt
//!   caching ([`CacheBreakpoint`]), OAuth login ([`AuthMethod::Bearer`])
//!   and, with the `batch` feature, message batches (`BatchProvider`).
//! - **OpenAI Chat Completions API** ([`OpenAi`]): OpenAI and every
//!   server that speaks it, such as Ollama, vLLM, LM Studio, the llama.cpp
//!   server, `OpenRouter`, Groq and Mistral. A request with a
//!   [`CacheBreakpoint`] is refused with [`Error::Unsupported`].
//!
//! # Claude on Bedrock
//!
//! Amazon Bedrock serves the Anthropic Messages API unchanged at
//! `https://bedrock-runtime.{region}.amazonaws.com/anthropic`.
//! [`Anthropic::bedrock`] (or [`AnthropicBuilder::bedrock`]) sends there with
//! a Bedrock API key as `x-api-key`; the model is a Bedrock model id or a
//! cross-region inference profile such as [`Bedrock::EU_CLAUDE_HAIKU_5_5`].
//! Asking, streaming, typed answers and tools work as on the Anthropic API;
//! message batches are [`Error::Unsupported`]. Bedrock API keys can expire,
//! and Bedrock rejects an invalid key with 403, an [`ErrorKind::Auth`] error. Bedrock's
//! OpenAI-compatible route, `…amazonaws.com/openai/v1`, serves other models
//! (such as Kimi K3) through [`OpenAi`] with the same key as its
//! [`credential`](OpenAiBuilder::credential).
//!
//! # One question, a typed answer
//!
//! `ask` on any provider sends one prompt and returns the text; `request`
//! starts a [`Request`] that adds a system prompt, a model, a token limit
//! or [`tools`](Request::tools). [`Request::stream`] yields the answer's
//! text as it is written and [`Request::events`] every [`StreamEvent`];
//! [`Request::into_request`] gives the full [`CompletionRequest`].
//! [`Request::send_as`] and, with the `schema` feature,
//! `Request::send_json` parse the answer into a type of yours; `send_as`
//! names the JSON Schema subset the providers accept.
//!
//! # Credentials
//!
//! A credential is passed in as an [`AuthMethod`] (API key or OAuth bearer
//! token) and never looked up: the crate reads no environment variable and
//! no credential file. [`Anthropic`] fails without one; [`OpenAi`]
//! sends an `authorization` header only with a credential, from
//! [`OpenAi::new`] or the builder's [`credential`](OpenAiBuilder::credential).
//! `AuthMethod`'s `Debug` output
//! masks the secret, and so does each client's.
//!
//! # Where requests go
//!
//! [`OpenAi::new`] goes to `https://api.openai.com/v1`; [`OpenAi::at`] goes
//! to the server it names and nowhere else, so a local setup never falls
//! through to a hosted API. [`Anthropic`] defaults to
//! `https://api.anthropic.com`; [`AnthropicBuilder::base_url`] names
//! another origin.
//!
//! # Timeouts and retry
//!
//! [`ProviderOptions`] sets the connect timeout (default 30 s) and the
//! longest silence on a response (default 90 s). [`RetryPolicy`] sets the
//! attempts per request (default 4); [`RetryPolicy::none`] turns retry off,
//! through the builders' `retry` setters:
//! `Anthropic::builder(key, model).retry(RetryPolicy::none()).build()`.
//! For a provider named in a configuration file, the `config` feature adds
//! `ProviderConfig` and `create_provider`.
//!
//! # Size caps
//!
//! An error body is read up to 64 KiB. An SSE line, an SSE event and the
//! streamed input of one tool call are each capped at 1 MiB; a larger one
//! is an [`Oversize`] error.
//!
//! ```no_run
//! use llm_provider::provider::{Anthropic, LlmProvider};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let provider: Box<dyn LlmProvider> = Box::new(Anthropic::new("sk-ant-…", Anthropic::CLAUDE_SONNET_5_5));
//! let answer = provider.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod anthropic;
mod api_error;
mod ask;
mod auth;
#[cfg(feature = "batch")]
mod batch;
mod bedrock;
mod chat_protocol;
mod completion;
mod endpoint;
mod error;
mod event;
mod llm;
mod message;
mod openai;
mod options;
mod request;
mod retry;
#[cfg(feature = "schema")]
mod schema;
#[cfg(test)]
pub(crate) mod test_stub;
#[cfg(test)]
pub(crate) mod test_support;
mod text_stream;
mod traits;

use std::pin::Pin;
use tokio_stream::Stream;

pub use anthropic::{Anthropic, AnthropicBuilder};
pub use api_error::ApiError;
pub use ask::{Ask, Request};
pub use auth::AuthMethod;
#[cfg(feature = "batch")]
pub use batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
pub use bedrock::Bedrock;
pub(crate) use chat_protocol::ChatProtocol;
#[cfg(feature = "agent")]
pub(crate) use completion::fold;
pub use completion::{Completion, ToolUseCall, Usage};
pub use error::{Error, ErrorKind, Oversize, RequestError};
pub use event::{StopReason, StreamEvent};
pub use llm::Llm;
pub use message::{Content, Message, MessageRole, ToolDefinition};
pub use openai::{OpenAi, OpenAiBuilder};
pub use options::ProviderOptions;
pub use request::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, ModelSelection,
    ResponseSchema,
};
pub use retry::RetryPolicy;
#[cfg(feature = "schema")]
pub(crate) use schema::derive as derive_schema;
pub use text_stream::TextStream;
pub use traits::{LlmProvider, ProviderInfo};

/// The stream of events a provider returns for one request; the first error
/// ends it.
pub type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;
