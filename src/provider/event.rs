/// Events from the LLM provider stream.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StreamEvent {
    /// Text chunk from the model.
    ContentDelta(String),
    /// Model wants to call a tool.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Model finished its response.
    MessageStop,
    /// Token usage stats. The two cache counters report prompt-cache
    /// writes and reads; providers without caching report 0.
    Usage {
        input_tokens: u32,
        output_tokens: u32,
        cache_creation_input_tokens: u32,
        cache_read_input_tokens: u32,
    },
    /// Why the model stopped. Emitted once, right before `MessageStop`.
    Stop(StopReason),
}

/// Why a completion ended, as reported by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Refusal,
    PauseTurn,
    /// A stop value this crate does not know.
    Unknown,
}
