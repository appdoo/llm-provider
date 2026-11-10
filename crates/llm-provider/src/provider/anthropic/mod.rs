//! The Anthropic Messages API: the [`Anthropic`] endpoint, which hands out
//! models (and, with the `batch` feature, message batches).
//!
//! ```no_run
//! use llm_provider::Anthropic;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let sonnet = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
//! let answer = sonnet.ask("Name three primes.").await?;
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

#[cfg(feature = "batch")]
pub use batch::Batches;
pub use builder::AnthropicBuilder;
pub use client::Anthropic;

#[cfg(test)]
mod test_support;
