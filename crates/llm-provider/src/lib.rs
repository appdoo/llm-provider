//! A Rust API for LLMs: Anthropic, Amazon Bedrock, OpenAI and
//! OpenAI-compatible servers (Ollama, vLLM). Ask, stream, typed answers,
//! tools and agents.
//!
//! # Quick tour
//!
//! An endpoint hands out a model; a model answers questions.
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_HAIKU_5_5);
//! let answer = claude.ask("What is a borrow checker?").await?;
//! println!("{answer}");
//! # Ok(()) }
//! ```
//!
//! Stream the answer as it is written (`futures::StreamExt` is your own
//! import):
//!
//! ```no_run
//! use futures::StreamExt;
//! use llm_provider::Llm;
//!
//! # async fn run(claude: &Llm) -> Result<(), llm_provider::Error> {
//! let mut text = claude.request("Tell a short story about a lighthouse.").stream();
//! while let Some(delta) = text.next().await {
//!     print!("{}", delta?);
//! }
//! # Ok(()) }
//! ```
//!
//! With the `schema` feature the answer parses into your type; its doc
//! comments are what the model reads:
//!
//! ```no_run
//! # #[cfg(feature = "schema")]
//! # async fn run(claude: &llm_provider::Llm) -> Result<(), llm_provider::Error> {
//! use llm_provider::JsonSchema;
//! use serde::Deserialize;
//!
//! /// What a text is about.
//! #[derive(Deserialize, JsonSchema)]
//! struct Summary {
//!     /// Three to five key points.
//!     key_points: Vec<String>,
//! }
//!
//! let summary: Summary = claude.request("Summarize: …").send_json().await?;
//! # let _ = summary; Ok(()) }
//! ```
//!
//! With the `agent` feature, `#[tool]` turns an async function into a tool
//! and a `Session` runs a turn, calling it when the model asks:
//!
//! ```no_run
//! # #[cfg(feature = "agent")]
//! # async fn run(claude: llm_provider::Llm) -> Result<String, Box<dyn std::error::Error>> {
//! use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
//!
//! use llm_provider::agent::{tool, Session, Tools};
//!
//! /// Current time in seconds since 1970-01-01 UTC.
//! #[tool]
//! async fn unix_time() -> Result<String, SystemTimeError> {
//!     Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string())
//! }
//!
//! let mut agent = Session::new(claude, Tools::new().with(UnixTime));
//! let turn = agent.run_turn("What is the Unix time now?").await?;
//! Ok(turn.response)
//! # }
//! ```
//!
//! # Features
//!
//! Only the first row is on by default.
//!
//! | Feature | Module | Adds dependencies |
//! |---|---|---|
//! | none | [`provider`] | the base set |
//! | `schema` | `ResponseSchema::of`, `Request::send_json` | `schemars` |
//! | `agent` | `agent` | `futures`, `llm-provider-macros`, `uuid` |
//! | `fs-tools` | `fs_tools` | `glob`, `regex`, `tempfile`, `walkdir` |
//! | `batch` | `BatchProvider` | none |
//!
//! `agent` turns on `schema`; `fs-tools` turns on `agent`. The base set is
//! `reqwest` (rustls), `tokio`, `serde`, `serde_json`, `thiserror`,
//! `async-trait`, `secrecy` (credentials), `backon` (retry schedule) and a
//! few small stream and byte crates.
//!
//! # Where to read on
//!
//! - [`config`]: models named by values, by hand or from a configuration
//!   file, through [`Llm::builder`] and [`Embedder::builder`].
//! - [`provider`]: protocols, credentials, typed answers, timeouts, retry
//!   and size caps.
//! - [`core`]: the kit for an own provider.
//! - `agent`: the turn loop, `#[tool]` and `#[tools]`, advisors, agents as
//!   tools, and the turn limits.
//! - `fs_tools`: the file tools and what their sandbox covers.
//!
//! The [`examples/`](https://github.com/appdoo/llm-provider/tree/main/crates/llm-provider/examples)
//! directory of the repository holds a runnable program for each topic.
#![warn(clippy::pedantic)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(unreachable_pub)]
#![forbid(unsafe_code)]

// The macros name `::llm_provider` paths; this lets them expand inside the
// crate too.
extern crate self as llm_provider;

pub mod provider;

pub mod config;
pub mod core;
mod transport;

#[cfg(feature = "agent")]
pub mod agent;

#[cfg(feature = "fs-tools")]
pub mod fs_tools;

pub use config::{ModelBuilder, Protocol};
pub use provider::{
    Anthropic, Ask, AuthMethod, Bedrock, CacheTtl, Completion, CompletionRequest, Embed, Embedder,
    Error, ErrorKind, EventStream, Llm, LlmProvider, Message, OpenAi, Prices, ProviderInfo,
    ProviderOptions, RawLlm, RawRequest, Request, ResponseSchema, RetryPolicy, Sampling,
    StreamEvent, TextStream, ToolDefinition, Usage,
};

/// Derives the JSON schema of a type, for
/// [`send_json`](crate::Request::send_json) and tool inputs; re-exported
/// from `schemars`.
#[cfg(feature = "schema")]
pub use schemars::JsonSchema;

/// The Rust blocks of the README, compiled as doctests so that a snippet
/// that no longer builds fails the tests.
#[cfg(all(doctest, feature = "fs-tools"))]
#[doc = include_str!("../../../README.md")]
pub struct ReadmeDoctests;

/// The Rust blocks of the crate README, the crates.io page, compiled as
/// doctests for the same reason.
#[cfg(all(doctest, feature = "fs-tools"))]
#[doc = include_str!("../README.md")]
pub struct CrateReadmeDoctests;

/// What the code of `#[tool]` and `#[tools]` names; not part of the public
/// API.
#[cfg(feature = "agent")]
#[doc(hidden)]
pub mod __private {
    pub use crate::agent::macro_support::{empty_schema, input_schema, output, parse_input};
    pub use async_trait::async_trait;
    pub use serde_json;
}
