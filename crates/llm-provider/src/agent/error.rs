//! [`SessionError`]: why an agent turn failed.

use crate::agent::runtime::TurnSummary;
use crate::provider::Error;

/// Why a [`Session::run_turn`](crate::agent::Session::run_turn)
/// or [`run_turn_cancellable`](crate::agent::Session::run_turn_cancellable)
/// failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
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
    /// The turn was cancelled by its token. `turn` is the work done before
    /// the cancel: all text of the turn so far, earlier iterations included,
    /// and the tool calls that completed (a call cut by the cancel is not
    /// listed). The interrupted reply's text is not appended to the history,
    /// and the history is as it was before the call.
    #[error("interrupted")]
    #[non_exhaustive]
    Interrupted {
        /// The turn so far: its text, finished tool calls, model calls and usage.
        turn: TurnSummary,
    },
}

impl SessionError {
    /// The work of the failed turn (text, finished tool calls, model calls,
    /// usage), for every variant. The history is unchanged.
    #[must_use]
    pub fn turn(&self) -> &TurnSummary {
        match self {
            Self::Model { turn, .. }
            | Self::MaxIterations { turn, .. }
            | Self::Truncated { turn }
            | Self::Interrupted { turn } => turn,
        }
    }
}
