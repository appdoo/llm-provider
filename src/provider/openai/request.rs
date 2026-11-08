//! The `/chat/completions` request body, built from a
//! [`CompletionRequest`]. Cache breakpoints have no counterpart and are
//! dropped.

use serde::Serialize;

use crate::provider::error::Error;
use crate::provider::message::{Content, Message, MessageRole, ToolDefinition};
use crate::provider::request::{CompletionRequest, ModelSelection};

/// The streaming request body.
#[derive(Debug, Serialize)]
pub(crate) struct ChatBody<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    max_tokens: u32,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatTool<'a>>,
}

/// Asks the server for a final usage chunk.
#[derive(Debug, Serialize)]
struct StreamOptions {
    include_usage: bool,
}

/// One message of the body, keyed by its `role`.
#[derive(Debug, Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub(crate) enum ChatMessage<'a> {
    /// The system prompt, first.
    System { content: &'a str },
    /// The text blocks of a user message, joined by a blank line.
    User { content: String },
    /// The model's text (`null` when it only called tools) and tool calls.
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ChatToolCall<'a>>,
    },
    /// One tool result.
    Tool {
        tool_call_id: &'a str,
        content: String,
    },
}

/// A tool call of an assistant message, keyed by its `type`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum ChatToolCall<'a> {
    /// A function call; `arguments` is the input as a JSON string.
    Function {
        id: &'a str,
        function: FunctionCall<'a>,
    },
}

/// The function a [`ChatToolCall`] names.
#[derive(Debug, Serialize)]
pub(crate) struct FunctionCall<'a> {
    name: &'a str,
    arguments: String,
}

/// A tool the model may call, keyed by its `type`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub(crate) enum ChatTool<'a> {
    /// A function with a JSON Schema for its parameters.
    Function { function: FunctionDefinition<'a> },
}

/// The function a [`ChatTool`] offers.
#[derive(Debug, Serialize)]
pub(crate) struct FunctionDefinition<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

/// The `content` of the tool message for a failed result; a struct keeps
/// the field order `error`, `content`.
#[derive(Debug, Serialize)]
struct ToolErrorContent<'a> {
    error: bool,
    content: &'a str,
}

impl<'a> ChatBody<'a> {
    /// The body for `request`; `model` is used for
    /// [`ModelSelection::Default`].
    ///
    /// # Errors
    /// [`Error::Json`] when a tool input or error result does not serialise.
    pub(crate) fn new(request: &'a CompletionRequest, model: &'a str) -> Result<Self, Error> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
