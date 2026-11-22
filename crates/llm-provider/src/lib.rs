//! A Rust API for LLMs: Anthropic, Amazon Bedrock, OpenAI and
//! OpenAI-compatible servers (Ollama, vLLM). Ask, stream, typed answers,
//! tools and agents.
//!
//! # Building blocks
//!
//! Six steps, each building on the one before; each is a chapter of the
//! [guide], with a runnable program in `examples/`.
//!
//! An endpoint hands out a model; a model answers questions
//! ([chapter 1](guide::ch01_ask)):
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
//! import; [chapter 2](guide::ch02_stream)):
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
//! comments are what the model reads ([chapter 3](guide::ch03_typed_answer)):
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
//! and a `Session` runs a turn, calling it when the model asks
//! ([chapter 4](guide::ch04_tool)):
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
//! An advisor is one more tool: a stronger model that reads the whole
//! conversation when the worker asks ([chapter 5](guide::ch05_advisor)):
//!
//! ```no_run
//! # #[cfg(feature = "agent")]
//! # async fn run(opus: llm_provider::Llm, stronger: llm_provider::Llm) -> Result<String, Box<dyn std::error::Error>> {
//! use llm_provider::agent::{Advisor, Session, Tools, CONSULT_ADVISOR};
//!
//! let tools = Tools::new().with(Advisor::new(stronger));
//! let mut coder = Session::builder(opus, tools).system(CONSULT_ADVISOR).build();
//! Ok(coder.run_turn("Why does median() return the wrong value?").await?.response)
//! # }
//! ```
//!
//! An agent becomes another agent's tool with `into_tool`; it keeps its own
//! conversation ([chapter 6](guide::ch06_agents_that_ask_each_other)):
//!
//! ```no_run
//! # #[cfg(feature = "agent")]
//! # async fn run(opus: llm_provider::Llm, haiku: llm_provider::Llm) -> Result<String, Box<dyn std::error::Error>> {
//! use llm_provider::agent::{Session, Tools};
//!
//! let researcher = Session::new(haiku, Tools::new())
//!     .into_tool("researcher", "Answers one factual question. Give it the question.");
//! let mut lead = Session::new(opus, Tools::new().with(researcher));
//! Ok(lead.run_turn("Ask the researcher who built the first lighthouse.").await?.response)
//! # }
//! ```
//!
//! # Guide
//!
//! The [guide] has one chapter per capability: the call, what it returns,
//! the knobs, the errors and an example. Chapters 1 to 6 are the blocks
//! above in depth.
//!
//! 1 [Ask](guide::ch01_ask) ·
//! 2 [Stream](guide::ch02_stream) ·
//! 3 [A typed answer](guide::ch03_typed_answer) ·
//! 4 [A tool](guide::ch04_tool) ·
//! 5 [An advisor](guide::ch05_advisor) ·
//! 6 [Agents that ask each other](guide::ch06_agents_that_ask_each_other) ·
//! 7 [Endpoints](guide::ch07_endpoints) ·
//! 8 [Prompt caching and cost](guide::ch08_caching_and_cost) ·
//! 9 [The agent loop and tool errors](guide::ch09_agent_loop) ·
//! 10 [Store and restore a session](guide::ch10_store_and_restore) ·
//! 11 [Compaction](guide::ch11_compaction) ·
//! 12 [Cancellation](guide::ch12_cancellation) ·
//! 13 [Sampling, timeouts and retry](guide::ch13_sampling_timeouts_retry) ·
//! 14 [Models from configuration, the explicit form](guide::ch14_configuration) ·
//! 15 [Sandboxed file tools](guide::ch15_file_tools) ·
//! 16 [Embeddings and batches](guide::ch16_embeddings_and_batches) ·
//! 17 [MCP servers as tools](guide::ch17_mcp_servers) ·
//! 18 [Your own provider](guide::ch18_own_provider).
//!
//! Where to find each capability beyond the blocks:
//!
//! | Capability | The call | Chapter |
//! |---|---|---|
//! | Other endpoints | `Bedrock::new(key, region)?.anthropic()`, `OpenAi::new(key)`, `OpenAi::at(url)?` | [7](guide::ch07_endpoints) |
//! | Prompt caching, cost | `.cache(CacheTtl::OneHour)`, on by default in the agent loop; `usage.cost(&Prices::per_million(3.0, 15.0))` | [8](guide::ch08_caching_and_cost) |
//! | The agent loop, a bad or hung tool call | `Session::new(llm, tools).run_turn(..)`; a failed tool call becomes an error result for the model, `.tool_timeout(..)` | [9](guide::ch09_agent_loop) |
//! | Store and restore a session | `.events(tx)`, `.restore(events)` | [10](guide::ch10_store_and_restore) |
//! | Compaction | `.compact_when(tokens, callback)` | [11](guide::ch11_compaction) |
//! | Cancellation | `run_turn_cancellable`, `send_cancellable` | [12](guide::ch12_cancellation) |
//! | Sampling, timeouts, retry | `.sampling(Sampling::new())`, `.timeouts(ProviderOptions)`, `.retry(RetryPolicy)` | [13](guide::ch13_sampling_timeouts_retry) |
//! | Models from configuration, the explicit form | `Llm::builder(model)`, `OpenAi::builder(url)?` | [14](guide::ch14_configuration) |
//! | Sandboxed file tools | `Files::new(Sandbox::new(root)?)` (`fs-tools`) | [15](guide::ch15_file_tools) |
//! | Embeddings, batches | `OpenAi::embedder(model)`, `Embed::embed`; `Anthropic::batches(model)` (`batch`) | [16](guide::ch16_embeddings_and_batches) |
//! | MCP servers as tools | `McpServer::stdio(..).connect()` in the crate `llm-provider-tools` | [17](guide::ch17_mcp_servers) |
//! | Your own provider | `core::Transport`, `core::decode` | [18](guide::ch18_own_provider) |
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
pub mod guide;
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
