//! [`ConversationRuntime`]: the turn loop over one provider and one
//! [`Toolbox`].

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::{stream, FutureExt, StreamExt};

use crate::agent::builder::{OnDelta, RuntimeBuilder, Settings};
use crate::agent::error::RuntimeError;
use crate::agent::tool::{ToolOutput, Toolbox};
use crate::provider::{
    fold, Completion, CompletionRequest, Content, Error, LlmProvider, Message, MessageRole,
    ModelSelection, StopReason, ToolUseCall,
};

/// What one turn did: the model's text, and how many tool calls and model
/// calls it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
        self.messages.push(Message::user(prompt));
        let mut turn = TurnSummary::default();

        loop {
            if turn.iterations == self.settings.max_iterations {
                return Err(RuntimeError::MaxIterations(self.settings.max_iterations));
            }
            turn.iterations += 1;

            let reply = self.next_reply().await?;
            turn.response.push_str(&reply.text);
            if reply.stop == StopReason::MaxTokens {
                return Err(RuntimeError::Truncated {
                    partial_text: turn.response,
                });
            }

            if reply.tool_uses.is_empty() {
                self.messages.push(Message::assistant(reply.text));
                return Ok(turn);
            }

            turn.tool_calls += reply.tool_uses.len();
            self.run_tools(reply.tool_uses).await;
        }
    }

    /// Sends the history to the model and folds its reply, handing each
    /// text delta to the `on_delta` receiver.
    async fn next_reply(&mut self) -> Result<Completion, Error> {
        let request = CompletionRequest {
            model: ModelSelection::Default,
            system: self.settings.system.clone(),
            messages: Arc::from(self.messages.as_slice()),
            tools: self.toolbox.definitions(),
            max_tokens: self.settings.max_tokens,
            cache_breakpoints: Vec::new(),
        };

        let on_delta = &mut self.on_delta;
        fold(self.provider.stream(request), |text| {
            if let Some(on_delta) = on_delta.as_mut() {
                on_delta(text);
            }
        })
        .await
    }

    /// Runs `calls`, at most `tool_concurrency` at once, and appends the
    /// calls and their results, in call order, to the history.
    async fn run_tools(&mut self, calls: Vec<ToolUseCall>) {
        let cap = self.settings.max_tool_output_chars;
        let runs: Vec<_> = calls
            .iter()
            .map(|call| call_tool(&self.toolbox, call, cap))
            .collect();
        let results: Vec<Content> = stream::iter(runs)
            .buffered(self.settings.tool_concurrency.max(1))
            .collect()
            .await;

        let uses = calls.into_iter().map(|call| Content::ToolUse {
            id: call.id,
            name: call.name,
            input: call.input,
        });
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: uses.collect(),
        });
        self.messages.push(Message {
            role: MessageRole::User,
            content: results,
        });
    }
}

/// Runs one tool call through `toolbox` and turns its output, or its
/// panic, into a tool result capped at `cap` characters.
async fn call_tool<T: Toolbox>(toolbox: &T, call: &ToolUseCall, cap: usize) -> Content {
    let run = AssertUnwindSafe(toolbox.call(&call.name, call.input.clone())).catch_unwind();
    let output = run.await.unwrap_or_else(|_| ToolOutput {
        content: format!("tool {} panicked", call.name),
        is_error: true,
    });

    let output = cap_output(output, cap);
    Content::ToolResult {
        tool_use_id: call.id.clone(),
        content: output.content,
        is_error: output.is_error,
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
    let count = output.content.chars().count();
    if count <= cap {
        return output;
    }

    let head: String = output.content.chars().take(cap / 2).collect();
    let tail: String = output
        .content
        .chars()
        .skip(count - (cap - cap / 2))
        .collect();
    ToolOutput {
        content: format!("{head}\n[… {} characters omitted …]\n{tail}", count - cap),
        is_error: output.is_error,
    }
}

#[cfg(test)]
mod tests;
