//! Helpers the Anthropic tests share: the API under test, a request and an
//! event-stream body.

use std::sync::Arc;

use super::api::AnthropicApi;
use super::builder::AnthropicBuilder;
use crate::provider::auth::AuthMethod;
use crate::provider::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, Message,
    MessageRole, ModelSelection, ProviderOptions, ToolDefinition,
};

/// A recorded event stream answering "Hi".
pub(super) const TEXT_SSE: &str = include_str!("../../../tests/fixtures/anthropic_text.sse");

/// One attempt against the API at `url` for `claude-sonnet-4-6`, key
/// `test-key`, with `options`.
pub(super) fn test_api(url: &str, options: ProviderOptions) -> AnthropicApi {
    AnthropicBuilder::new(
        AuthMethod::from("test-key"),
        "claude-sonnet-4-6".to_string(),
    )
    .base_url(url)
    .timeouts(options)
    .api()
}

/// A user message with one text block.
pub(super) fn user_text(text: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: vec![Content::Text {
            text: text.to_string(),
        }],
    }
}

/// A breakpoint at `position` with `ttl`.
pub(super) fn bp(position: BreakpointPosition, ttl: CacheTtl) -> CacheBreakpoint {
    CacheBreakpoint { position, ttl }
}

/// A request with a system prompt, one tool, one user message and
/// `breakpoints`.
pub(super) fn text_request(breakpoints: Vec<CacheBreakpoint>) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: Some("S".to_string()),
        messages: Arc::from(vec![user_text("Count from 1 to 50")]),
        tools: vec![ToolDefinition {
            name: "t".to_string(),
            description: "d".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }],
        max_tokens: 5,
        cache_breakpoints: breakpoints,
        response_schema: None,
    }
}

/// An event stream with input usage 3, the text "1, 2", output usage 5 and
/// `stop_reason`.
pub(super) fn sse_body(stop_reason: &str) -> String {
    format!(
        "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"usage\":{{\"input_tokens\":3}}}}}}\n\n\
         event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"1, 2\"}}}}\n\n\
         event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"{stop_reason}\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}
