//! The parts of a Messages API response that the stream decoder and the
//! batch results share: stop reason, usage, content blocks, a whole message
//! and an error object.

use serde::Deserialize;

use crate::provider::error::ApiError;
use crate::provider::event::StopReason;
use crate::provider::traits::{Completion, Usage};

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
        todo!()
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
}

impl From<WireUsage> for Usage {
    fn from(usage: WireUsage) -> Self {
        todo!()
    }
}

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

/// A whole response message, as a batch result carries it.
#[derive(Debug, Deserialize)]
pub(crate) struct ResponseMessage {
    content: Vec<ResponseBlock>,
    #[serde(default)]
    stop_reason: Option<WireStopReason>,
    #[serde(default)]
    usage: WireUsage,
}

impl From<ResponseMessage> for Completion {
    /// Text blocks joined, tool uses in order; no stop reason reads as
    /// [`StopReason::EndTurn`].
    fn from(message: ResponseMessage) -> Self {
        todo!()
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
        todo!()
    }
}
