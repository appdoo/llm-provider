//! [`Error`]: why a connection to an MCP server failed.

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
}
