//! The conversation a request carries: [`Message`]s of [`Content`] blocks,
//! and the [`ToolDefinition`]s the model may call.

use serde::{Deserialize, Serialize};

/// One message of a conversation: who sent it and its content blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Who sent the message.
    pub role: MessageRole,
    /// The blocks of the message, in order.
    pub content: Vec<Content>,
}

impl Message {
    /// A user message of one text block.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: vec![Content::Text { text: text.into() }],
        }
    }

    /// An assistant message of one text block.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: vec![Content::Text { text: text.into() }],
        }
    }
}

/// The sender of a [`Message`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// The caller, including tool results it hands back.
    User,
    /// The model.
    Assistant,
}

/// One block of a [`Message`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Content {
    /// Plain text.
    Text {
        /// The text.
        text: String,
    },
    /// A tool call the model made.
    ToolUse {
        /// The call's id, which its result refers to.
        id: String,
        /// The name of the tool called.
        name: String,
        /// The tool's input as the model wrote it.
        input: serde_json::Value,
    },
    /// The result of a tool call, sent back to the model.
    ToolResult {
        /// The id of the [`ToolUse`](Self::ToolUse) this answers.
        tool_use_id: String,
        /// The tool's output.
        content: String,
        /// Whether the tool failed.
        is_error: bool,
    },
}

/// A tool the model may call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// The name the model calls the tool by.
    pub name: String,
    /// What the tool does, for the model.
    pub description: String,
    /// The JSON Schema of the tool's input.
    pub input_schema: serde_json::Value,
}

#[cfg(test)]
mod tests;
