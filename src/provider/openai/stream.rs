//! Decoding a `/chat/completions` stream into [`StreamEvent`]s: `data:`
//! lines of JSON chunks, `data: [DONE]` at the end.

use serde::Deserialize;

use crate::provider::api_error::EnvelopeError;
use crate::provider::completion::Usage;
use crate::provider::error::Error;
use crate::provider::openai::tool_calls::{ToolCallFragment, ToolCalls};
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent};
use crate::transport::{ByteStream, LineAccumulator};

/// Decodes a response body into events. A body without bytes is
/// [`Error::EmptyResponse`]; the first error ends the stream. The first
/// finish reason is emitted as [`StreamEvent::Stop`] right before
/// [`StreamEvent::MessageStop`] at `[DONE]`.
pub(crate) fn decode(bytes: ByteStream) -> EventStream {
    todo!()
}

/// Turns `data:` lines into events.
#[derive(Debug, Default)]
struct ChunkDecoder {
    calls: ToolCalls,
    stop: Option<StopReason>,
    done: bool,
}

/// One streamed chunk: an error, or choices and usage.
#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    error: Option<EnvelopeError>,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

/// One choice of a chunk; only the first is read.
#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Option<Delta>,
    #[serde(default)]
    finish_reason: Option<FinishReason>,
}

/// The new text and tool-call fragments of a choice.
#[derive(Debug, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallFragment>>,
}

/// A `finish_reason`; values this crate does not know read as
/// [`Unknown`](Self::Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    #[serde(other)]
    Unknown,
}

impl From<FinishReason> for StopReason {
    fn from(reason: FinishReason) -> Self {
        match reason {
            FinishReason::Stop => StopReason::EndTurn,
            FinishReason::ToolCalls => StopReason::ToolUse,
            FinishReason::Length => StopReason::MaxTokens,
            FinishReason::ContentFilter => StopReason::Refusal,
            FinishReason::Unknown => StopReason::Unknown,
        }
    }
}

/// The `usage` of the final chunk; absent counters read as 0.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

/// The breakdown of the prompt tokens.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
struct PromptTokensDetails {
    cached_tokens: u32,
}

impl From<ChatUsage> for Usage {
    /// Cached prompt tokens are cache reads; `input_tokens` is the
    /// uncached rest.
    fn from(usage: ChatUsage) -> Self {
        let cached = usage
            .prompt_tokens_details
            .unwrap_or_default()
            .cached_tokens;

        Usage {
            input_tokens: usage.prompt_tokens.saturating_sub(cached),
            output_tokens: usage.completion_tokens,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: cached,
        }
    }
}

#[cfg(test)]
mod tests;
