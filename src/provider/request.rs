//! [`CompletionRequest`]: what a provider is asked to complete, with its
//! model choice and prompt-cache breakpoints.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::provider::message::{Message, ToolDefinition};

/// The model a request runs on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSelection {
    /// The model the provider was built with.
    Default,
    /// The model with this id.
    Named(String),
}

/// A request to complete a conversation.
///
/// `messages` is `Arc<[Message]>` so callers that keep their own history
/// clone the request without copying it; it serialises as a JSON array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// The model to run on.
    pub model: ModelSelection,
    /// The system prompt, if any.
    pub system: Option<String>,
    /// The conversation so far.
    pub messages: Arc<[Message]>,
    /// The tools the model may call.
    pub tools: Vec<ToolDefinition>,
    /// The output token limit.
    pub max_tokens: u32,
    /// Prompt-cache breakpoints; none means no caching. An empty list is
    /// left out of the JSON.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<CacheBreakpoint>,
}

/// A prompt-cache breakpoint: cache the prefix up to and including
/// `position` for `ttl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheBreakpoint {
    /// Where the cached prefix ends.
    pub position: BreakpointPosition,
    /// How long the entry lives.
    pub ttl: CacheTtl,
}

/// Where in the request a cache breakpoint sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointPosition {
    /// After the last tool definition.
    Tools,
    /// After the system prompt.
    System,
    /// After the last content block of `messages[i]`.
    Message(usize),
}

/// The lifetime of a prompt-cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CacheTtl {
    /// Five minutes.
    FiveMinutes,
    /// One hour.
    OneHour,
}

#[cfg(test)]
mod tests;
