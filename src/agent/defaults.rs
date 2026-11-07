//! `LoopDefaults` — the tunable constants of the agent turn loop.

/// Defaults consumed by `ConversationRuntime`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDefaults {
    /// Per-request `max_tokens` sent to the provider.
    pub max_tokens: u32,
    /// Maximum tool-loop iterations before `RuntimeError::MaxIterations`.
    pub max_iterations: usize,
    /// Parallel tool-call fan-out cap enforced by the semaphore in
    /// `ConversationRuntime::run_turn`.
    pub tool_concurrency: usize,
    /// Longest tool output, in characters, kept in a tool result; longer output keeps its head and tail.
    pub max_tool_output_chars: usize,
}

impl Default for LoopDefaults {
    fn default() -> Self {
        Self {
            max_tokens: 8192,
            max_iterations: 25,
            tool_concurrency: 4,
            max_tool_output_chars: 100_000,
        }
    }
}
