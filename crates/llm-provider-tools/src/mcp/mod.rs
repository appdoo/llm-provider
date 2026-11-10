//! MCP servers as tools: connect to a server over stdio or Streamable HTTP,
//! and every tool it lists becomes one [`Tool`](llm_provider::agent::Tool)
//! the agent can call.
//!
//! ```no_run
//! use llm_provider::agent::{ConversationRuntime, Tools};
//! use llm_provider::Anthropic;
//! use llm_provider_tools::mcp::McpServer;
//!
//! # async fn run(key: &str, docs_token: String) -> Result<(), Box<dyn std::error::Error>> {
//! let claude = Anthropic::new(key).llm("claude-sonnet-5-5");
//!
//! // A local server as a child process; its environment holds only what is passed.
//! let fetch = McpServer::stdio("uvx", ["mcp-server-fetch"]).connect().await?;
//!
//! // A remote server over Streamable HTTP with a bearer token.
//! let docs = McpServer::http("https://mcp.example.com/mcp")
//!     .bearer(docs_token)
//!     .connect()
//!     .await?;
//!
//! let tools = Tools::new().with_all(fetch).with_all(docs);
//! let mut agent = ConversationRuntime::new(claude, tools);
//! let turn = agent.run_turn("Summarize https://www.rust-lang.org in two sentences.").await?;
//! println!("{}", turn.response);
//! # Ok(())
//! # }
//! ```
//!
//! Tool descriptions and results come from the server and are untrusted:
//! the model reads them as data. A stdio server runs with the caller's
//! privileges; [`approve`](McpServer::approve) puts a person in front of
//! every call.

// The one file that names `rmcp`; the module name keeps its paths apart
// from the crate's.
#[path = "rmcp.rs"]
mod adapter;
mod error;
mod names;
mod reply;
mod server;
mod stderr;
mod tools;

pub use error::Error;
pub use server::{Http, McpServer, Stdio};
pub use tools::McpTools;
