//! Models named by values: a [`ModelBuilder`] takes the protocol, the
//! endpoint, the region and the model as values, by hand or read from a
//! configuration file through `serde`, and builds an [`Llm`](crate::Llm) or
//! an [`Embedder`](crate::Embedder). The credential is always set in code.
//!
//! Every [`build`](ModelBuilder::build) makes its own endpoint, so two models
//! built this way never share a connection pool. Code that knows its
//! endpoint at compile time builds [`Anthropic`](crate::Anthropic),
//! [`OpenAi`](crate::OpenAi) or [`Bedrock`](crate::Bedrock) directly.
//!
//! ```no_run
//! use llm_provider::{Llm, ModelBuilder};
//!
//! # async fn run(text: &str, key: String) -> Result<(), llm_provider::Error> {
//! let opus = serde_json::from_str::<ModelBuilder<Llm>>(text)?
//!     .api_key(key)
//!     .build()?;
//! let answer = opus.ask("Name three primes.").await?;
//! # let _ = answer; Ok(()) }
//! ```

mod build;
mod builder;
mod error;
mod protocol;

pub use builder::ModelBuilder;
pub use error::ConfigError;
pub use protocol::Protocol;

#[cfg(test)]
mod tests;
