//! Any OpenAI-compatible Chat Completions server (OpenAI, Ollama, LM
//! Studio, vLLM, OpenRouter): an [`OpenAiCompatible`] client that streams
//! completions.
//!
//! ```no_run
//! use llm_provider::OpenAiCompatible;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let ollama = OpenAiCompatible::new("http://localhost:11434/v1", "llama3.2");
//! let answer = ollama.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod api;
mod builder;
mod client;
mod request;
mod stream;
mod tool_calls;

pub use builder::OpenAiBuilder;
pub use client::OpenAiCompatible;
