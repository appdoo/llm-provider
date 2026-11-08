//! [`AgentTool`]: a [`ConversationRuntime`] another agent calls as a tool.

use std::fmt;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::agent::macro_support::{input_schema, parse_input};
use crate::agent::runtime::ConversationRuntime;
use crate::agent::tool::{Tool, ToolOutput, Toolbox, Transcript};
use crate::provider::{LlmProvider, ToolDefinition};

/// The input of an [`AgentTool`] call.
#[derive(Deserialize, JsonSchema)]
struct Prompt {
    /// What the agent is asked to do, as one self-contained message.
    prompt: String,
}

/// An agent as a tool, made by
/// [`ConversationRuntime::into_tool`]: a call with `{"prompt": "…"}` runs
/// one turn of the wrapped agent and answers with its response, or with the
/// turn's error marked as failed.
///
/// The agent keeps its conversation across calls. Calls to one agent run
/// one at a time, even under `tool_concurrency`; calls to different agents
/// run in parallel.
pub struct AgentTool<P, T> {
    name: String,
    description: String,
    agent: Mutex<ConversationRuntime<P, T>>,
}

impl<P: LlmProvider, T: Toolbox> AgentTool<P, T> {
    /// The tool [`ConversationRuntime::into_tool`] returns.
    pub(super) fn new(agent: ConversationRuntime<P, T>, name: String, description: String) -> Self {
        Self {
            name,
            description,
            agent: Mutex::new(agent),
        }
    }
}

#[async_trait]
impl<P: LlmProvider, T: Toolbox> Tool for AgentTool<P, T> {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: input_schema::<Prompt>(),
        }
    }

    async fn call(&self, input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
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
impl<P, T> fmt::Debug for AgentTool<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentTool")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
