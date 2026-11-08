//! [`Error`]: why a connection to an MCP server failed.

use std::time::Duration;

/// Why [`connect`](crate::mcp::McpServer::connect) failed. A failed tool
/// call is not an error: the tool answers the model with `is_error`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The server's program could not be started.
    #[error("could not start MCP server {program}: {source}")]
    Spawn {
        /// The program as the caller named it.
        program: String,
        /// Why the operating system refused.
        source: std::io::Error,
    },

    /// The server asked for credentials: the bearer token is missing or
    /// was refused.
    #[error("MCP server {server} requires authorization")]
    Auth {
        /// The program or URL of the server.
        server: String,
    },

    /// The connection or the `initialize` handshake failed.
    #[error("could not connect to MCP server {server}: {source}")]
    Connect {
        /// The program or URL of the server.
        server: String,
        /// What the MCP client reported.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The server did not answer `tools/list`.
    #[error("could not list the tools of MCP server {server}: {source}")]
    List {
        /// The program or URL of the server.
        server: String,
        /// What the MCP client reported.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The server did not finish `initialize` and `tools/list` within the
    /// [`timeout`](crate::mcp::McpServer::timeout).
    #[error("MCP server {server} did not connect within {}s", timeout.as_secs_f64())]
    Timeout {
        /// The program or URL of the server.
        server: String,
        /// The timeout that ran out.
        timeout: Duration,
    },

    /// A tool's name, as the model would see it, is longer than the 64
    /// characters a model provider accepts.
    #[error("tool {tool} of MCP server {server} would be named {name}, longer than 64 characters")]
    NameTooLong {
        /// The program or URL of the server.
        server: String,
        /// The tool's name on the server.
        tool: String,
        /// The name the model would see.
        name: String,
    },

    /// Two tools of one server get the same name once characters a model
    /// provider refuses are replaced.
    #[error("tools {} of MCP server {server} would both be named {name}", tools.join(" and "))]
    NameCollision {
        /// The program or URL of the server.
        server: String,
        /// The tools' names on the server.
        tools: Vec<String>,
        /// The name they share.
        name: String,
    },
}
