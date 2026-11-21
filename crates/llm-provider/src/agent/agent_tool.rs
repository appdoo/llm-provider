//! [`AgentTool`]: a [`Session`] another agent calls as a tool.

use std::fmt;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::agent::macro_support::{input_schema, parse_input};
use crate::agent::runtime::Session;
use crate::agent::tool::{Tool, ToolOutput, Toolbox};
use crate::agent::CallContext;
use crate::provider::ToolDefinition;

/// The input of an [`AgentTool`] call.
#[derive(Deserialize, JsonSchema)]
struct Prompt {
    /// What the agent is asked to do, as one self-contained message.
    prompt: String,
}

/// An agent as a tool, made by
/// [`Session::into_tool`]: a call with `{"prompt": "…"}` runs
/// one turn of the wrapped agent and answers with its response, or with the
/// turn's error marked as failed.
///
/// The agent keeps its conversation across calls. Calls to one agent run
/// one at a time, even under `tool_concurrency`; calls to different agents
/// run in parallel.
pub struct AgentTool<T> {
    name: String,
    description: String,
    agent: Mutex<Session<T>>,
}

impl<T: Toolbox> AgentTool<T> {
    /// The tool [`Session::into_tool`] returns.
    pub(super) fn new(agent: Session<T>, name: String, description: String) -> Self {
        Self {
            name,
            description,
            agent: Mutex::new(agent),
        }
    }
}

#[async_trait]
impl<T: Toolbox> Tool for AgentTool<T> {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: input_schema::<Prompt>(),
        }
    }

    async fn call(&self, input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
        let input = match parse_input::<Prompt>(input) {
            Ok(input) => input,
            Err(output) => return output,
        };

        let mut agent = self.agent.lock().await;
        match agent.run_turn(&input.prompt).await {
            Ok(turn) => ToolOutput {
                content: turn.response,
                is_error: false,
            },
            Err(error) => ToolOutput {
                content: error.to_string(),
                is_error: true,
            },
        }
    }
}

// Hand-written: the wrapped agent is behind a lock; its name stands for it.
impl<T> fmt::Debug for AgentTool<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentTool")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
