//! [`StreamEvent`]: what a provider's stream yields, and [`StopReason`].

use crate::provider::completion::{ToolUseCall, Usage};

/// One event of a provider's stream.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StreamEvent {
    /// A piece of the answer text.
    ContentDelta(String),
    /// A complete tool call.
    ToolUse(ToolUseCall),
    /// The end of the answer.
    MessageStop,
    /// Token counts; the events of one answer add up.
    Usage(Usage),
    /// Why the model stopped; sent once, right before
    /// [`MessageStop`](Self::MessageStop).
    Stop(StopReason),
}

impl StreamEvent {
    /// A [`ContentDelta`](Self::ContentDelta) of `text`.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::ContentDelta(text.into())
    }
}

/// Why a completion ended, as the provider reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    /// The model finished its answer.
    EndTurn,
    /// The model wants tool results.
    ToolUse,
    /// The output reached the token limit.
    MaxTokens,
    /// The output hit a stop sequence.
    StopSequence,
    /// The model declined to answer.
    Refusal,
    /// The server paused a long turn.
    PauseTurn,
    /// A stop value this crate does not know.
    Unknown,
}
