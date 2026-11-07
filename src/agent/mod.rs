//! Level 1: the agent turn loop — stream a completion, run the requested
//! tools, feed the results back, repeat until the model stops.

mod defaults;
mod runtime;
pub mod tool;

pub use defaults::LoopDefaults;
pub use runtime::{ConversationRuntime, RuntimeError, ToolExecResult, ToolExecutor, TurnSummary};

/// Errors that can occur during tool execution.
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
