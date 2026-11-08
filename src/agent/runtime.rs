//! [`ConversationRuntime`]: the turn loop over one provider and one
//! [`Toolbox`].

use std::fmt;

use crate::agent::builder::{OnDelta, RuntimeBuilder, Settings};
use crate::agent::error::RuntimeError;
use crate::agent::tool::{ToolOutput, Toolbox};
use crate::provider::{Content, LlmProvider, Message, ToolUseCall};

/// What one turn did: the model's text, and how many tool calls and model
/// calls it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSummary {
    /// The text of every model reply in the turn, joined.
    pub response: String,
    /// How many tool calls the turn ran.
    pub tool_calls: usize,
    /// How many model calls the turn made.
    pub iterations: usize,
}

/// An agent: a provider, the tools it may call, and the conversation so
/// far. Each [`run_turn`](Self::run_turn) adds the prompt, the replies and
/// the tool results to the history.
///
/// ```no_run
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::agent::{ConversationRuntime, Tools};
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::fs_tools::{EditTool, ReadTool, Sandbox};
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::OpenAiCompatible;
///
/// # #[cfg(feature = "fs-tools")]
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");
///
///     let sandbox = Sandbox::new("docs")?;
///     let tools = Tools::new()
///         .with(ReadTool::new(sandbox.clone()))
///         .with(EditTool::new(sandbox));
///
///     let mut agent = ConversationRuntime::builder(model, tools)
///         .system("You fix typos in the files you are given.")
///         .max_iterations(10)
///         .on_delta(|text| print!("{text}"))
///         .build();
///
///     let turn = agent.run_turn("Fix the typos in README.md.").await?;
///     eprintln!("{} tool calls", turn.tool_calls);
///     Ok(())
/// }
/// # #[cfg(not(feature = "fs-tools"))]
/// # fn main() {}
/// ```
pub struct ConversationRuntime<P, T> {
    provider: P,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
    messages: Vec<Message>,
}

impl<P: LlmProvider, T: Toolbox> ConversationRuntime<P, T> {
    /// A builder for a runtime on `provider` that offers the tools in
    /// `toolbox`.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAiCompatible;
    ///
    /// let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");
    /// let agent = ConversationRuntime::builder(model, Tools::new())
    ///     .system("You answer in one sentence.")
    ///     .max_tokens(1024)
    ///     .build();
    /// # let _ = agent;
    /// ```
    pub fn builder(provider: P, toolbox: T) -> RuntimeBuilder<P, T> {
        RuntimeBuilder::new(provider, toolbox)
    }

    /// A runtime with no system prompt, `max_tokens` 8192, 25 model calls
    /// per turn, 4 tool calls at once and tool results capped at 100 000
    /// characters.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAiCompatible;
    ///
    /// let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");
    /// let agent = ConversationRuntime::new(model, Tools::new());
    /// # let _ = agent;
    /// ```
    pub fn new(provider: P, toolbox: T) -> Self {
        Self::builder(provider, toolbox).build()
    }

    /// The runtime [`RuntimeBuilder::build`] assembles.
    pub(super) fn from_parts(
        provider: P,
        toolbox: T,
        settings: Settings,
        on_delta: Option<OnDelta>,
    ) -> Self {
        Self {
            provider,
            toolbox,
            settings,
            on_delta,
            messages: Vec::new(),
        }
    }

    /// Runs one turn: sends `prompt`, runs the tools the model asks for,
    /// feeds their results back, and repeats until the model answers
    /// without a tool call.
    ///
    /// # Errors
    /// [`RuntimeError::Provider`] when a model call fails,
    /// [`RuntimeError::Truncated`] when a reply stops at `max_tokens`, and
    /// [`RuntimeError::MaxIterations`] when the model still asks for tools
    /// after the last allowed call.
    pub async fn run_turn(&mut self, prompt: &str) -> Result<TurnSummary, RuntimeError> {
        todo!()
    }

    /// Runs one tool call and turns its output into a capped tool result.
    async fn call_tool(&self, call: &ToolUseCall) -> Content {
        todo!()
    }
}

// Hand-written: the provider, toolbox and callback need not be `Debug`.
impl<P, T> fmt::Debug for ConversationRuntime<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConversationRuntime")
            .field("settings", &self.settings)
            .field("messages", &self.messages.len())
            .finish_non_exhaustive()
    }
}

/// `output` with its head and tail kept when it is longer than `cap`
/// characters.
fn cap_output(output: ToolOutput, cap: usize) -> ToolOutput {
    todo!()
}

#[cfg(test)]
mod tests;
