//! The Messages API request body, rendered from a [`CompletionRequest`].
//! Rendering validates the cache breakpoints, so an invalid layout never
//! reaches the wire.

use serde::Serialize;

use crate::provider::error::RequestError;
use crate::provider::traits::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, Message,
    MessageRole, ModelSelection, ToolDefinition,
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
        validate_breakpoints(request)?;
        let model = match &request.model {
            ModelSelection::Default => default_model,
            ModelSelection::Named(id) => id,
        };
        let mut body = Self {
            model,
            max_tokens: request.max_tokens,
            system: request.system.as_deref().map(System::Text),
            messages: request.messages.iter().map(WireMessage::from).collect(),
            tools: request.tools.iter().map(WireTool::from).collect(),
            stream: false,
        };
        for breakpoint in &request.cache_breakpoints {
            body.mark(breakpoint);
        }
        Ok(body)
    }

    /// Puts the `cache_control` marker of `breakpoint` on its target: the
    /// last tool, the system prompt (as one text block) or the last block of
    /// a message. The target exists; [`validate_breakpoints`] checked it.
    fn mark(&mut self, breakpoint: &CacheBreakpoint) {
        let marker = Some(CacheControl::from(breakpoint.ttl));
        match breakpoint.position {
            BreakpointPosition::Tools => {
                if let Some(tool) = self.tools.last_mut() {
                    tool.cache_control = marker;
                }
            }
            BreakpointPosition::System => {
                if let Some(System::Text(text)) = self.system {
                    let block = WireBlock {
                        content: BlockContent::Text { text },
                        cache_control: marker,
                    };
                    self.system = Some(System::Blocks(vec![block]));
                }
            }
            BreakpointPosition::Message(index) => {
                let message = self.messages.get_mut(index);
                if let Some(block) = message.and_then(|m| m.content.last_mut()) {
                    block.cache_control = marker;
                }
            }
        }
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

impl<'a> From<&'a Message> for WireMessage<'a> {
    fn from(message: &'a Message) -> Self {
        WireMessage {
            role: message.role.into(),
            content: message.content.iter().map(WireBlock::from).collect(),
        }
    }
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

impl<'a> From<&'a Content> for WireBlock<'a> {
    fn from(content: &'a Content) -> Self {
        WireBlock {
            content: content.into(),
            cache_control: None,
        }
    }
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
    let breakpoints = &request.cache_breakpoints;
    if breakpoints.len() > MAX_CACHE_BREAKPOINTS {
        return Err(RequestError::TooManyBreakpoints {
            count: breakpoints.len(),
            max: MAX_CACHE_BREAKPOINTS,
        });
    }
    for (n, breakpoint) in breakpoints.iter().enumerate() {
        check_target(request, breakpoint)?;
        if breakpoints[..n]
            .iter()
            .any(|earlier| earlier.position == breakpoint.position)
        {
            return Err(RequestError::DuplicateBreakpoint(breakpoint.position));
        }
    }
    check_ttl_order(breakpoints)
}

/// Checks that `breakpoint` names a target the request has.
fn check_target(
    request: &CompletionRequest,
    breakpoint: &CacheBreakpoint,
) -> Result<(), RequestError> {
    match breakpoint.position {
        BreakpointPosition::Message(index) => match request.messages.get(index) {
            None => Err(RequestError::BreakpointOutOfRange { index }),
            Some(message) if message.content.is_empty() => {
                Err(RequestError::BreakpointOnEmptyMessage { index })
            }
            Some(_) => Ok(()),
        },
        BreakpointPosition::System if request.system.is_none() => {
            Err(RequestError::BreakpointWithoutSystem)
        }
        BreakpointPosition::Tools if request.tools.is_empty() => {
            Err(RequestError::BreakpointWithoutTools)
        }
        _ => Ok(()),
    }
}

/// Checks that every one-hour breakpoint comes before every five-minute one
/// in the order tools, system, messages, whatever the order of the list.
fn check_ttl_order(breakpoints: &[CacheBreakpoint]) -> Result<(), RequestError> {
    let ranks = |ttl: CacheTtl| {
        breakpoints
            .iter()
            .filter(move |breakpoint| breakpoint.ttl == ttl)
            .map(|breakpoint| rank(breakpoint.position))
    };
    let Some(first_short) = ranks(CacheTtl::FiveMinutes).min() else {
        return Ok(());
    };
    if ranks(CacheTtl::OneHour).any(|long| long > first_short) {
        return Err(RequestError::TtlOrder);
    }
    Ok(())
}

/// The place of `position` in the order the API reads a request: tools,
/// system, then the messages in turn.
fn rank(position: BreakpointPosition) -> usize {
    match position {
        BreakpointPosition::Tools => 0,
        BreakpointPosition::System => 1,
        BreakpointPosition::Message(index) => 2 + index,
    }
}
