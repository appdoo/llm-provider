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
use super::names::model_names;

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
    /// Lists every tool of `session` (all pages) and names each one for
    /// the model, `<prefix>_<name>` when `prefix` is set, sanitised by
    /// [`model_names`].
    pub(crate) async fn list(
        session: Session,
        prefix: Option<String>,
        timeout: Duration,
        approve: Option<Approve>,
    ) -> Result<Self, Error> {
        let definitions = session.list_tools().await?;
        let remotes: Vec<String> = definitions.iter().map(|tool| tool.name.clone()).collect();
        let names = model_names(session.server(), prefix.as_deref(), &remotes)?;
        let connection = Arc::new(Connection {
            session,
            timeout,
            approve,
        });

        let tools = definitions
            .into_iter()
            .zip(names)
            .map(|(definition, name)| McpTool::new(&connection, name, definition))
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
    /// The tool `definition` of the server behind `connection`, shown to
    /// the model as `name`; calls still use the server's own name.
    fn new(connection: &Arc<Connection>, name: String, definition: ToolDefinition) -> Self {
        let remote = definition.name.clone();
        let mut definition = definition;
        definition.name = name;
        Self {
            connection: Arc::clone(connection),
            remote,
            definition,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn call(&self, input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        let name = &self.definition.name;
        let connection = &self.connection;
        if let Some(approve) = &connection.approve {
            if !approved(approve, name, &input).await {
                return failure(format!("call to {name} declined"));
            }
        }

        let arguments = match arguments(name, input) {
            Ok(arguments) => arguments,
            Err(output) => return output,
        };
        let call = connection.session.call(&self.remote, arguments);
        match tokio::time::timeout(connection.timeout, call).await {
            Ok(Ok(reply)) => reply.into_output(),
            Ok(Err(reason)) => failure(format!("call to {name} failed: {reason}")),
            Err(_) => failure(format!(
                "call to {name} timed out after {}s",
                connection.timeout.as_secs_f64()
            )),
        }
    }
}

/// Whether `approve` allows the call to `name` with `input`. It runs on a
/// blocking thread, since it may wait for a person; a panic in it declines.
async fn approved(approve: &Approve, name: &str, input: &serde_json::Value) -> bool {
    let approve = Arc::clone(approve);
    let (name, input) = (name.to_owned(), input.clone());
    tokio::task::spawn_blocking(move || approve(&name, &input))
        .await
        .unwrap_or(false)
}

/// The arguments of a call to `name`: the input object, or `None` for a
/// `null` input; any other input answers `is_error`.
fn arguments(
    name: &str,
    input: serde_json::Value,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, ToolOutput> {
    match input {
        serde_json::Value::Object(object) => Ok(Some(object)),
        serde_json::Value::Null => Ok(None),
        _ => Err(failure(format!("input to {name} is not a JSON object"))),
    }
}

/// A failed call's output: `is_error` with `content`.
fn failure(content: String) -> ToolOutput {
    ToolOutput::error(content)
}

#[cfg(test)]
mod tests;
