//! The core our providers are built on, public so that yours can be too: a
//! [`Transport`] that POSTs a JSON body with your headers and retries before
//! the first content event, and the [`decode`] functions that turn the reply
//! into [`StreamEvent`](crate::StreamEvent)s.
//!
//! # Build your own provider
//!
//! An own provider is a type that implements [`LlmProvider`](crate::LlmProvider) by sending a
//! body through a [`Transport`] and decoding the reply with [`decode`]. The
//! example speaks Ollama's native `/api/generate` (NDJSON):
//!
//! ```no_run
//! use llm_provider::core::{decode, Transport};
//! use llm_provider::provider::{
//!     CompletionRequest, EventStream, LlmProvider, ProviderInfo, StreamEvent,
//! };
//! use llm_provider::Llm;
//! use serde_json::json;
//!
//! struct Ollama {
//!     transport: Transport,
//!     model: String,
//! }
//!
//! impl LlmProvider for Ollama {
//!     // The reply as events: one text event per NDJSON line.
//!     fn stream(&self, request: CompletionRequest) -> EventStream {
//!         let prompt = request.last_user_text().unwrap_or_default();
//!         let body = json!({ "model": self.model, "prompt": prompt, "stream": true });
//!         self.transport.send_body("/api/generate", body, |bytes| {
//!             decode::lines(bytes, |line| {
//!                 let v: serde_json::Value = serde_json::from_str(line)?;
//!                 Ok(v["response"].as_str().map(StreamEvent::text))
//!             })
//!         })
//!     }
//!
//!     // Who this provider is, for errors and logs.
//!     fn info(&self) -> ProviderInfo {
//!         ProviderInfo::new("ollama", &self.model)
//!     }
//! }
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let transport = Transport::at("http://localhost:11434")?; // .bearer(key) when needed
//! let llm = Llm::from_provider(Ollama { transport, model: "qwen3".into() });
//! println!("{}", llm.ask("How many goats?").await?);
//! # Ok(()) }
//! ```
//!
//! The same program is
//! [`examples/own_provider.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/own_provider.rs).
//! When the server speaks the OpenAI or Anthropic wire, use
//! [`decode::openai_chat`] or [`decode::anthropic`] in place of `lines`.

pub mod decode;
mod transport;

pub use crate::transport::ByteStream;
pub use transport::Transport;
