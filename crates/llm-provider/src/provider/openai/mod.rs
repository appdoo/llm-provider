//! Any OpenAI-compatible Chat Completions server (OpenAI, Ollama, LM
//! Studio, vLLM, OpenRouter): an [`OpenAi`] client that hands out one
//! [`Llm`](crate::provider::Llm) per model.
//!
//! ```no_run
//! use llm_provider::OpenAi;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let ollama = OpenAi::at("http://localhost:11434/v1").llm("llama3.2");
//! let answer = ollama.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod api;
mod builder;
mod client;
mod embeddings;
mod request;
mod stream;
mod tool_calls;

pub use builder::OpenAiBuilder;
pub use client::OpenAi;
