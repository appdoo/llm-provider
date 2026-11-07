//! The Messages API request body, rendered from a [`CompletionRequest`].
//! Rendering validates the cache breakpoints, so an invalid layout never
//! reaches the wire.

use serde::Serialize;

use crate::provider::error::RequestError;
use crate::provider::traits::{
    CacheBreakpoint, CacheTtl, CompletionRequest, Content, MessageRole, ToolDefinition,
};

/// The most cache breakpoints the Messages API accepts per request.
pub(crate) const MAX_CACHE_BREAKPOINTS: usize = 4;

/// The JSON body of a Messages API request, borrowed from the
/// [`CompletionRequest`] it renders.
#[derive(Debug, Serialize)]
pub(crate) struct MessagesBody<'a> {
    model: &'a str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<System<'a>>,
    messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool<'a>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

impl<'a> MessagesBody<'a> {
    /// The body for `request`; `default_model` stands in for
    /// [`ModelSelection::Default`](crate::provider::ModelSelection::Default).
    /// Each cache breakpoint becomes a `cache_control` marker on its target,
    /// and an invalid breakpoint layout is a [`RequestError`].
    pub(crate) fn new(
        request: &'a CompletionRequest,
        default_model: &'a str,
    ) -> Result<Self, RequestError> {
        todo!()
    }

    /// The same body with `"stream": true`.
    pub(crate) fn streaming(mut self) -> Self {
        self.stream = true;
        self
    }
}

/// The `system` field: a plain string, or one text block when the system
/// prompt carries a cache breakpoint.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum System<'a> {
    Text(&'a str),
    Blocks(Vec<WireBlock<'a>>),
}

/// One message of the conversation.
#[derive(Debug, Serialize)]
struct WireMessage<'a> {
    role: WireRole,
    content: Vec<WireBlock<'a>>,
}

/// The sender of a message.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum WireRole {
    User,
    Assistant,
}

impl From<MessageRole> for WireRole {
    fn from(role: MessageRole) -> Self {
        match role {
            MessageRole::User => WireRole::User,
            MessageRole::Assistant => WireRole::Assistant,
        }
    }
}

/// One content block, with the cache marker when a breakpoint ends on it.
#[derive(Debug, Serialize)]
struct WireBlock<'a> {
    #[serde(flatten)]
    content: BlockContent<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

/// What a content block holds.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BlockContent<'a> {
    Text {
        text: &'a str,
    },
    ToolUse {
        id: &'a str,
        name: &'a str,
        input: &'a serde_json::Value,
    },
    ToolResult {
        tool_use_id: &'a str,
        content: &'a str,
        is_error: bool,
    },
}

impl<'a> From<&'a Content> for BlockContent<'a> {
    fn from(content: &'a Content) -> Self {
        match content {
            Content::Text { text } => BlockContent::Text { text },
            Content::ToolUse { id, name, input } => BlockContent::ToolUse { id, name, input },
            Content::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => BlockContent::ToolResult {
                tool_use_id,
                content,
                is_error: *is_error,
            },
        }
    }
}

/// One tool the model may call, with the cache marker on the last tool when
/// the tools carry a breakpoint.
#[derive(Debug, Serialize)]
struct WireTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

impl<'a> From<&'a ToolDefinition> for WireTool<'a> {
    fn from(tool: &'a ToolDefinition) -> Self {
        WireTool {
            name: &tool.name,
            description: &tool.description,
            input_schema: &tool.input_schema,
            cache_control: None,
        }
    }
}

/// A `cache_control` marker: `{"type":"ephemeral"}`, plus `"ttl":"1h"` for
/// the one-hour cache (which needs no beta header).
#[derive(Debug, Clone, Copy, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<&'static str>,
}

impl From<CacheTtl> for CacheControl {
    fn from(ttl: CacheTtl) -> Self {
        let ttl = match ttl {
            CacheTtl::FiveMinutes => None,
            CacheTtl::OneHour => Some("1h"),
        };
        CacheControl {
            kind: "ephemeral",
            ttl,
        }
    }
}

/// Checks the breakpoints against the request: at most
/// [`MAX_CACHE_BREAKPOINTS`], each on an existing non-empty target, no
/// position twice, and no one-hour breakpoint after a five-minute one in the
/// order tools, system, messages.
fn validate_breakpoints(request: &CompletionRequest) -> Result<(), RequestError> {
    todo!()
}

/// Checks that `breakpoint` names a target the request has.
fn check_target(
    request: &CompletionRequest,
    breakpoint: &CacheBreakpoint,
) -> Result<(), RequestError> {
    todo!()
}

/// Checks that every one-hour breakpoint comes before every five-minute one.
fn check_ttl_order(breakpoints: &[CacheBreakpoint]) -> Result<(), RequestError> {
    todo!()
}
