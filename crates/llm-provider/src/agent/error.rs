//! [`RuntimeError`]: why an agent turn failed.

use crate::agent::runtime::{ToolCall, TurnSummary};
use crate::provider::Error;

/// Why a [`ConversationRuntime::run_turn`](crate::agent::ConversationRuntime::run_turn)
/// or [`run_turn_cancellable`](crate::agent::ConversationRuntime::run_turn_cancellable)
/// failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The model failed: the request, the stream, or a stream that ended
    /// before its last event. `turn` is the work done before the failure.
    #[error("model error: {error}")]
    #[non_exhaustive]
    Model {
        /// What the model call failed with.
        error: Error,
        /// The turn so far: its text, tool calls, model calls and usage.
        turn: TurnSummary,
    },
    /// The model still asked for tools after `limit` model calls.
    #[error("Max iterations ({limit}) exceeded")]
    #[non_exhaustive]
    MaxIterations {
        /// The most model calls one turn may make.
        limit: usize,
        /// The turn so far: its text, tool calls, model calls and usage.
        turn: TurnSummary,
    },
    /// The reply was cut at `max_tokens`. `turn.response` is the text of
    /// every iteration of the turn.
    #[error("reply truncated at max_tokens")]
    #[non_exhaustive]
    Truncated {
        /// The turn so far, the cut reply's text and usage included.
        turn: TurnSummary,
    },
    /// The turn was cancelled by its token. `partial_text` is all text of the
    /// turn so far, earlier iterations included; `tool_calls` lists the calls
    /// that completed before the cancel. The interrupted reply's text is not
    /// appended to the history, and the history is as it was before the call.
    #[error("interrupted")]
    #[non_exhaustive]
    Interrupted {
        /// The text the model produced in this turn before the cancel.
        partial_text: String,
        /// The tool calls that completed in this turn before the cancel.
        tool_calls: Vec<ToolCall>,
    },
}

impl RuntimeError {
    /// The work of the failed turn (text, tool calls, model calls, usage),
    /// or `None` for [`Interrupted`](Self::Interrupted), which reports its
    /// text and tool calls itself. The history is unchanged either way.
    #[must_use]
    pub fn turn(&self) -> Option<&TurnSummary> {
        match self {
            Self::Model { turn, .. }
            | Self::MaxIterations { turn, .. }
            | Self::Truncated { turn } => Some(turn),
            Self::Interrupted { .. } => None,
        }
    }
}
