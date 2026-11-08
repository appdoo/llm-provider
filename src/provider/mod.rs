//! The provider layer: the [`LlmProvider`] trait, the [`Anthropic`] and
//! [`OpenAiCompatible`] clients, the one-question [`Request`] and retry.
//!
//! ```no_run
//! use llm_provider::provider::{Anthropic, LlmProvider};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let provider: Box<dyn LlmProvider> = Box::new(Anthropic::new("sk-ant-…", "claude-sonnet-4-6"));
//! let answer = provider.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod anthropic;
mod api_error;
mod ask;
mod auth;
#[cfg(feature = "batch")]
mod batch;
mod completion;
mod error;
mod event;
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
mod traits;

use std::pin::Pin;
use tokio_stream::Stream;

pub use anthropic::{Anthropic, AnthropicBuilder};
pub use api_error::ApiError;
pub use ask::Request;
pub use auth::AuthMethod;
#[cfg(feature = "batch")]
pub use batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
#[cfg(feature = "agent")]
pub(crate) use completion::fold;
pub use completion::{Completion, ToolUseCall, Usage};
pub use error::{Error, ErrorKind, Oversize, RequestError};
pub use event::{StopReason, StreamEvent};
pub use message::{Content, Message, MessageRole, ToolDefinition};
pub use openai::{OpenAiBuilder, OpenAiCompatible};
pub use options::ProviderOptions;
pub use request::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, ModelSelection,
    ResponseSchema,
};
pub use retry::RetryPolicy;
#[cfg(feature = "schema")]
pub(crate) use schema::derive as derive_schema;
pub use traits::{LlmProvider, ProviderInfo};

/// The stream of events a provider returns for one request; the first error
/// ends it.
pub(crate) type EventStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, Error>> + Send>>;
