#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)]
#![deny(unreachable_pub)]
#![forbid(unsafe_code)]

// The macros name `::llm_provider` paths; this lets them expand inside the
// crate too.
extern crate self as llm_provider;

pub mod provider;

#[cfg(feature = "config")]
pub mod config;
mod transport;

#[cfg(feature = "agent")]
pub mod agent;

#[cfg(feature = "fs-tools")]
pub mod fs_tools;

pub use provider::{
    Anthropic, Ask, AuthMethod, Completion, CompletionRequest, Error, ErrorKind, LlmProvider,
    Message, OpenAiCompatible, ProviderOptions, Request, ResponseSchema, RetryPolicy,
};

/// Derives the JSON schema of a type, for
/// [`send_json`](crate::Request::send_json) and tool inputs; re-exported
/// from `schemars`.
#[cfg(feature = "schema")]
pub use schemars::JsonSchema;

/// What the code of `#[tool]` and `#[tools]` names; not part of the public
/// API.
#[cfg(feature = "agent")]
#[doc(hidden)]
pub mod __private {
    pub use crate::agent::macro_support::{empty_schema, input_schema, output, parse_input};
    pub use async_trait::async_trait;
    pub use serde_json;
}
