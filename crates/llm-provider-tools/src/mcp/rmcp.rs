//! The adapter to the `rmcp` MCP client, the only place its types appear:
//! open a [`Session`] over stdio or Streamable HTTP, list its tools, call
//! one.

use llm_provider::provider::ToolDefinition;
use rmcp::model::{ClientCapabilities, ClientConfig, Implementation};
use rmcp::service::{RoleClient, RunningService};

use super::error::Error;
use super::reply::Reply;
use super::server::{Http, Stdio};

/// An open MCP client session; dropping it ends the session and stops a
/// spawned server.
pub(crate) struct Session {
    service: RunningService<RoleClient, ClientConfig>,
    server: String,
}

impl Session {
    /// Starts `stdio`'s program with its cleared environment, relays its
    /// stderr under `label`, and runs `initialize`.
    pub(crate) async fn spawn(stdio: &Stdio, label: String) -> Result<Self, Error> {
        let _ = (stdio, label);
        todo!()
    }

    /// Opens a Streamable HTTP connection to `http`, with its bearer token,
    /// and runs `initialize`.
    pub(crate) async fn open(http: &Http) -> Result<Self, Error> {
        let _ = http;
        todo!()
    }

    /// Every tool the server lists, all pages: name, description and input
    /// schema unchanged.
    pub(crate) async fn list_tools(&self) -> Result<Vec<ToolDefinition>, Error> {
        let _ = (&self.service, &self.server);
        todo!()
    }

    /// Calls the server's tool `name` with `arguments`. `Err` carries a
    /// short reason: a protocol error or a lost connection.
    pub(crate) async fn call(
        &self,
        name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<Reply, String> {
        let _ = (name, arguments);
        todo!()
    }
}

/// What this client tells a server about itself: its name and version, and
/// no capabilities.
fn client_config() -> ClientConfig {
    ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
    )
}
