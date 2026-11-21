//! Tools from outside for [`llm-provider`](llm_provider) agents.
//!
//! - [`mcp`]: the tools of any MCP server, stdio or Streamable HTTP, as
//!   [`Tool`](llm_provider::agent::Tool)s an agent adds with one line.
//!
//! ```no_run
//! use llm_provider::agent::Tools;
//! use llm_provider_tools::mcp::McpServer;
//!
//! # async fn run() -> Result<(), llm_provider_tools::mcp::Error> {
//! let fetch = McpServer::stdio("uvx", ["mcp-server-fetch"]).connect().await?;
//! let tools = Tools::new().with_all(fetch);
//! # let _ = tools;
//! # Ok(())
//! # }
//! ```
#![warn(clippy::pedantic)]
#![warn(missing_docs)]

pub mod mcp;
