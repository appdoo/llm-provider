//! [`RuntimeError`]: why an agent turn failed.

use crate::provider::Error;

/// Why a [`ConversationRuntime::run_turn`](crate::agent::ConversationRuntime::run_turn)
/// failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The provider failed: the request, the stream, or a stream that ended
    /// before its last event.
    #[error("Provider error: {0}")]
    Provider(#[from] Error),
    /// The model still asked for tools after this many model calls.
    #[error("Max iterations ({0}) exceeded")]
    MaxIterations(usize),
    /// The reply was cut at `max_tokens`. `partial_text` is the text of
    /// every iteration of the turn; it is not appended to the history.
    #[error("reply truncated at max_tokens")]
    Truncated {
        /// The text the model produced in this turn before the cut.
        partial_text: String,
    },
}
