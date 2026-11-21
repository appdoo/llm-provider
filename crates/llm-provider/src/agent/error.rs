//! [`RuntimeError`]: why an agent turn failed.

use crate::agent::runtime::ToolCall;
use crate::provider::Error;

/// Why a [`ConversationRuntime::run_turn`](crate::agent::ConversationRuntime::run_turn)
/// or [`run_turn_cancellable`](crate::agent::ConversationRuntime::run_turn_cancellable)
/// failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The model failed: the request, the stream, or a stream that ended
    /// before its last event.
    #[error("model error: {0}")]
    Model(#[from] Error),
    /// The model still asked for tools after this many model calls.
    #[error("Max iterations ({0}) exceeded")]
    MaxIterations(usize),
    /// The reply was cut at `max_tokens`. `partial_text` is the text of
    /// every iteration of the turn. The text of earlier iterations is in the
    /// history beside their tool calls; the cut reply's text is not appended.
    #[error("reply truncated at max_tokens")]
    Truncated {
        /// The text the model produced in this turn before the cut.
        partial_text: String,
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
