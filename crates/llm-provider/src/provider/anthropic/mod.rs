//! The Anthropic Messages API: an [`Anthropic`] client that streams
//! completions (and, with the `batch` feature, runs message batches).
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let claude = Anthropic::new("sk-ant-…", Anthropic::CLAUDE_SONNET_5_5);
//! let answer = claude.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod api;
#[cfg(feature = "batch")]
mod batch;
mod builder;
mod client;
mod output_schema;
mod request;
mod response;
mod stream;

pub use builder::AnthropicBuilder;
pub use client::Anthropic;

#[cfg(test)]
mod test_support;
