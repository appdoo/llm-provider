//! The Anthropic Messages API: an [`Anthropic`] client that streams
//! completions and runs message batches.
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…", "claude-sonnet-4-6");
//! let answer = claude.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod api;
mod batch;
mod builder;
mod client;
mod request;
mod response;
mod stream;

pub use builder::AnthropicBuilder;
pub use client::Anthropic;
pub(crate) use client::DEFAULT_MODEL;

#[cfg(test)]
mod tests;
