//! A small, pure-Rust toolbox for LLM agents, from one completion to a
//! tool-using agent loop.
//!
//! The front API is small: `Anthropic::new(key, model).ask(..)` answers one
//! question. Underneath sits the Anthropic Messages API in depth (prompt
//! caching, OAuth, structured outputs, batches) and every server that
//! speaks the OpenAI Chat Completions API. Agents are built from a few
//! pieces that compose: tools, agents as tools, and advisors.
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…", "claude-haiku-5-5");
//! let answer = claude.ask("What is a borrow checker?").await?;
//! println!("{answer}");
//! # Ok(()) }
//! ```
//!
//! # Levels
//!
//! Each level builds on the one before it. Only level 0 is on by default.
//!
//! | Level | Module | Feature | Adds dependencies |
//! |---|---|---|---|
//! | 0: providers | [`provider`] | always on | none beyond the base set |
//! | Derived answer schemas | `ResponseSchema::of`, `Request::send_json` | `schema` | `schemars` |
//! | 1: agent loop | `agent` | `agent` (turns on `schema`) | `futures`, `llm-provider-macros` |
//! | File tools | `fs_tools` | `fs-tools` (turns on `agent`) | `glob`, `regex`, `tempfile`, `walkdir` |
//! | Message batches | `BatchProvider` | `batch` | none |
//! | Configured providers | `config` | `config` | none |
//!
//! The base set is `reqwest` (rustls), `tokio`, `serde`, `serde_json`,
//! `thiserror`, `async-trait`, `secrecy` (credentials), `backon` (retry
//! schedule) and a few small stream and byte crates.
//!
//! # Where to read on
//!
//! - [`provider`]: protocols, credentials, typed answers, timeouts, retry
//!   and size caps.
//! - `agent`: the turn loop, `#[tool]` and `#[tools]`, advisors, agents as
//!   tools, and the turn limits.
//! - `fs_tools`: the file tools and what their sandbox covers.
//!
//! The `examples/` directory of the repository holds a runnable program
//! for each topic.
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
    Anthropic, Ask, AuthMethod, Completion, CompletionRequest, Error, ErrorKind, EventStream,
    LlmProvider, Message, OpenAiCompatible, ProviderOptions, Request, ResponseSchema, RetryPolicy,
    StreamEvent, TextStream,
};

/// Derives the JSON schema of a type, for
/// [`send_json`](crate::Request::send_json) and tool inputs; re-exported
/// from `schemars`.
#[cfg(feature = "schema")]
pub use schemars::JsonSchema;

/// The Rust blocks of the README, compiled as doctests so that a snippet
/// that no longer builds fails the tests.
#[cfg(all(doctest, feature = "fs-tools"))]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

/// What the code of `#[tool]` and `#[tools]` names; not part of the public
/// API.
#[cfg(feature = "agent")]
#[doc(hidden)]
pub mod __private {
    pub use crate::agent::macro_support::{empty_schema, input_schema, output, parse_input};
    pub use async_trait::async_trait;
    pub use serde_json;
}
