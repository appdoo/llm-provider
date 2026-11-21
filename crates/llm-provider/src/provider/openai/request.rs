//! The `/chat/completions` request body, built from a
//! [`CompletionRequest`]. Prompt caching has no counterpart, so a request
//! with cache breakpoints is refused; a typed answer becomes a `json_schema`
//! response format.

use serde::Serialize;

use crate::provider::error::Error;
use crate::provider::message::{Content, Message, MessageRole, ToolDefinition};
#[cfg(test)]
use crate::provider::request::ModelSelection;
use crate::provider::request::{CompletionRequest, ResponseSchema};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat<'a>>,
}

/// The shape the answer text must take, keyed by its `type`.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponseFormat<'a> {
    /// JSON that matches a schema.
    JsonSchema { json_schema: JsonSchemaFormat<'a> },
}

/// The schema of a [`ResponseFormat::JsonSchema`], not in strict mode.
#[derive(Debug, Serialize)]
struct JsonSchemaFormat<'a> {
    name: &'a str,
    schema: &'a serde_json::Value,
    strict: bool,
}

impl<'a> From<&'a ResponseSchema> for ResponseFormat<'a> {
    fn from(schema: &'a ResponseSchema) -> Self {
        ResponseFormat::JsonSchema {
            json_schema: JsonSchemaFormat {
                name: schema.name(),
                schema: schema.schema(),
                // Strict mode needs `additionalProperties: false` and every
                // property required, which a schemars schema lacks.
                strict: false,
            },
        }
    }
}

/// Asks the server for a final usage chunk.
#[derive(Debug, Serialize)]
pub(super) struct StreamOptions {
    pub(super) include_usage: bool,
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
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default).
    ///
    /// # Errors
    /// [`Error::Unsupported`] when `request` carries cache breakpoints;
    /// [`Error::Json`] when a tool input or error result does not serialise.
    pub(crate) fn new(request: &'a CompletionRequest, model: &'a str) -> Result<Self, Error> {
        if !request.cache_breakpoints.is_empty() {
            return Err(Error::Unsupported(
                "prompt caching is not available with OpenAI",
            ));
        }

        let model = request.model.resolve(model);

        Ok(Self {
            model,
            messages: messages(request)?,
            max_tokens: request.max_tokens,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            tools: request.tools.iter().map(ChatTool::from).collect(),
            response_format: request.response_schema.as_ref().map(ResponseFormat::from),
        })
    }
}

impl<'a> From<&'a ToolDefinition> for ChatTool<'a> {
    fn from(tool: &'a ToolDefinition) -> Self {
        ChatTool::Function {
            function: FunctionDefinition {
                name: &tool.name,
                description: &tool.description,
                parameters: &tool.input_schema,
            },
        }
    }
}

/// The system prompt, then each message of `request` in order.
fn messages(request: &CompletionRequest) -> Result<Vec<ChatMessage<'_>>, Error> {
    let mut out = Vec::new();
    if let Some(system) = &request.system {
        out.push(ChatMessage::System { content: system });
    }

    for message in request.messages.iter() {
        match message.role {
            MessageRole::User => out.extend(user_messages(message)?),
            MessageRole::Assistant => out.push(assistant_message(message)?),
        }
    }

    Ok(out)
}

/// Each tool result as its own `tool` message, in block order, then the
/// text blocks as one `user` message. A user message carries no tool calls
/// on this wire.
fn user_messages(message: &Message) -> Result<Vec<ChatMessage<'_>>, Error> {
    let mut out = Vec::new();
    let mut texts = Vec::new();

    for block in &message.content {
        match block {
            Content::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => out.push(ChatMessage::Tool {
                tool_call_id: tool_use_id,
                content: tool_result_content(content, *is_error)?,
            }),
            Content::Text { text } => texts.push(text.as_str()),
            Content::ToolUse { .. } => {}
        }
    }

    if !texts.is_empty() {
        out.push(ChatMessage::User {
            content: texts.join("\n\n"),
        });
    }

    Ok(out)
}

/// The text blocks joined (`null` when there are none) and each tool call.
/// An assistant message carries no tool results.
fn assistant_message(message: &Message) -> Result<ChatMessage<'_>, Error> {
    let mut texts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &message.content {
        match block {
            Content::Text { text } => texts.push(text.as_str()),
            Content::ToolUse { id, name, input } => tool_calls.push(ChatToolCall::Function {
                id,
                function: FunctionCall {
                    name,
                    arguments: serde_json::to_string(input)?,
                },
            }),
            Content::ToolResult { .. } => {}
        }
    }

    let content = (!texts.is_empty()).then(|| texts.join("\n\n"));
    Ok(ChatMessage::Assistant {
        content,
        tool_calls,
    })
}

/// A result's text; a failed one as the JSON string
/// `{"error":true,"content":"<text>"}`.
fn tool_result_content(content: &str, is_error: bool) -> Result<String, Error> {
    if !is_error {
        return Ok(content.to_string());
    }

    let wrapped = ToolErrorContent {
        error: true,
        content,
    };
    Ok(serde_json::to_string(&wrapped)?)
}

#[cfg(test)]
mod tests;
