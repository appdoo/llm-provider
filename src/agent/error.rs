//! The errors of the agent: [`RuntimeError`] for a turn, [`ToolError`] for
//! the file tools.

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

/// Why a file tool failed. It moves to `fs_tools` in wave C of Spec 36.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolError {
    /// An error occurred during tool execution.
    #[error("Tool execution error: {0}")]
    Execution(String),
    /// An I/O error occurred.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// A sandboxed path operation failed (escape, canonicalize, etc).
    #[error("Sandbox error: {0}")]
    Sandbox(String),
    /// Input exceeded size limits (e.g. Read without offset/limit on a large file).
    #[error("Input too large: {0}")]
    InputTooLarge(String),
}
