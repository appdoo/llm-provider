//! [`McpTools`]: the tools of one connected MCP server, and the
//! [`Tool`] each of them becomes: approval, timeout, call.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use llm_provider::agent::{IntoTools, Tool, ToolOutput, Transcript};
use llm_provider::provider::ToolDefinition;

use super::adapter::Session;
use super::error::Error;

/// The approval hook of [`McpServer::approve`](super::McpServer::approve).
pub(crate) type Approve = Arc<dyn Fn(&str, &serde_json::Value) -> bool + Send + Sync>;

/// The tools of one connected MCP server, ready for
/// [`Tools::with_all`](llm_provider::agent::Tools::with_all).
///
/// The connection lives as long as this value or any tool taken from it;
/// when the last one is dropped, the session ends and a spawned server
/// process is stopped.
pub struct McpTools {
    connection: Arc<Connection>,
    tools: Vec<McpTool>,
}

/// The session and the call options every tool of one server shares.
struct Connection {
    session: Session,
    timeout: Duration,
    approve: Option<Approve>,
}

/// One MCP tool as an agent [`Tool`]: the name the model sees, and the
/// name the server knows.
struct McpTool {
    connection: Arc<Connection>,
    remote: String,
    definition: ToolDefinition,
}

impl McpTools {
    /// Lists every tool of `session` (all pages) and names each one,
    /// `<prefix>_<name>` when `prefix` is set.
    pub(crate) async fn list(
        session: Session,
        prefix: Option<String>,
        timeout: Duration,
        approve: Option<Approve>,
    ) -> Result<Self, Error> {
        let remote = session.list_tools().await?;
        let connection = Arc::new(Connection {
            session,
            timeout,
            approve,
        });

        let tools = remote
            .into_iter()
            .map(|definition| McpTool::new(&connection, prefix.as_deref(), definition))
            .collect();
        Ok(Self { connection, tools })
    }

    /// The names of the tools, as the model sees them, in the server's
    /// order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tools
            .iter()
            .map(|tool| tool.definition.name.clone())
            .collect()
    }
}

// Hand-written: the session has no useful `Debug`; the tool names stand
// for it.
impl fmt::Debug for McpTools {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("McpTools").field(&self.names()).finish()
    }
}

impl IntoTools for McpTools {
    fn into_tools(self) -> Vec<Box<dyn Tool>> {
        drop(self.connection);
        self.tools
            .into_iter()
            .map(|tool| Box::new(tool) as Box<dyn Tool>)
            .collect()
    }
}

impl McpTool {
    /// The tool `definition` of the server behind `connection`, renamed
    /// `<prefix>_<name>` when `prefix` is set.
    fn new(connection: &Arc<Connection>, prefix: Option<&str>, definition: ToolDefinition) -> Self {
        let remote = definition.name.clone();
        let name = match prefix {
            Some(prefix) => format!("{prefix}_{remote}"),
            None => remote.clone(),
        };
        Self {
            connection: Arc::clone(connection),
            remote,
            definition: ToolDefinition { name, ..definition },
        }
    }

    /// The call's arguments: the input object, or `None` for no input.
    fn arguments(
        &self,
        input: serde_json::Value,
    ) -> Result<Option<serde_json::Map<String, serde_json::Value>>, ToolOutput> {
        let _ = input;
        todo!()
    }
}

#[async_trait]
impl Tool for McpTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        let _ = (input, &self.remote, &self.connection);
        todo!()
    }
}

/// A failed call's output: `is_error` with `content`.
fn failure(content: String) -> ToolOutput {
    ToolOutput {
        content,
        is_error: true,
    }
}

#[cfg(test)]
mod tests;
