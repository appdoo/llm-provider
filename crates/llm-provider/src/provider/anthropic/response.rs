//! The parts of a Messages API response that the stream decoder and the
//! batch results share: stop reason, usage and an error object, plus the
//! content blocks and whole message of a batch result.

use serde::Deserialize;

use crate::provider::api_error::ApiError;
use crate::provider::StopReason;
use crate::provider::Usage;
#[cfg(feature = "batch")]
use crate::provider::{Completion, ToolUseCall};

/// A `stop_reason` as the API writes it; values this crate does not know
/// read as [`Unknown`](Self::Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WireStopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Refusal,
    PauseTurn,
    #[serde(other)]
    Unknown,
}

impl From<WireStopReason> for StopReason {
    fn from(reason: WireStopReason) -> Self {
        match reason {
            WireStopReason::EndTurn => StopReason::EndTurn,
            WireStopReason::ToolUse => StopReason::ToolUse,
            WireStopReason::MaxTokens => StopReason::MaxTokens,
            WireStopReason::StopSequence => StopReason::StopSequence,
            WireStopReason::Refusal => StopReason::Refusal,
            WireStopReason::PauseTurn => StopReason::PauseTurn,
            WireStopReason::Unknown => StopReason::Unknown,
        }
    }
}

/// A `usage` object; absent counters read as 0.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_field_names)] // the API names every counter `*_tokens`
pub(crate) struct WireUsage {
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
    pub(crate) cache_creation_input_tokens: u32,
    pub(crate) cache_read_input_tokens: u32,
    pub(crate) cache_creation: Option<CacheCreation>,
}

/// The split of the cache writes by lifetime; only the one-hour share is read.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
pub(crate) struct CacheCreation {
    pub(crate) ephemeral_1h_input_tokens: u32,
}

impl From<WireUsage> for Usage {
    fn from(usage: WireUsage) -> Self {
        Usage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_creation_1h_input_tokens: usage
                .cache_creation
                .unwrap_or_default()
                .ephemeral_1h_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
        }
    }
}

#[cfg(feature = "batch")]
/// One content block of a whole message; block types this crate does not
/// read (such as `thinking`) are [`Other`](Self::Other) and skipped.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ResponseBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[cfg(feature = "batch")]
/// A whole response message, as a batch result carries it.
#[derive(Debug, Deserialize)]
pub(crate) struct ResponseMessage {
    content: Vec<ResponseBlock>,
    #[serde(default)]
    stop_reason: Option<WireStopReason>,
    #[serde(default)]
    usage: WireUsage,
}

#[cfg(feature = "batch")]
impl From<ResponseMessage> for Completion {
    /// Text blocks joined, tool uses in order; no stop reason reads as
    /// [`StopReason::EndTurn`].
    fn from(message: ResponseMessage) -> Self {
        let mut text = String::new();
        let mut tool_uses = Vec::new();
        for block in message.content {
            match block {
                ResponseBlock::Text { text: part } => text.push_str(&part),
                ResponseBlock::ToolUse { id, name, input } => {
                    tool_uses.push(ToolUseCall { id, name, input });
                }
                ResponseBlock::Other => {}
            }
        }

        Completion {
            text,
            tool_uses,
            usage: message.usage.into(),
            stop: message
                .stop_reason
                .map_or(StopReason::EndTurn, StopReason::from),
        }
    }
}

/// An `error` object of an in-stream event or an errored batch result.
#[derive(Debug, Deserialize)]
pub(crate) struct WireError {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
}

impl From<WireError> for ApiError {
    fn from(error: WireError) -> Self {
        ApiError::from_anthropic_event(&error.kind, error.message)
    }
}
