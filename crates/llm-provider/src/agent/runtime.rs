//! [`ConversationRuntime`]: the turn loop over one model and one
//! [`Toolbox`].

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::{stream, FutureExt, StreamExt};

use crate::agent::agent_tool::AgentTool;
use crate::agent::builder::{OnDelta, RuntimeBuilder, Settings};
use crate::agent::error::RuntimeError;
use crate::agent::tool::{ToolOutput, Toolbox, Transcript};
use crate::provider::{
    fold, Completion, CompletionRequest, Content, Error, Llm, LlmProvider, Message, MessageRole,
    ModelSelection, StopReason, ToolUseCall,
};

/// What one turn did: the model's text, every tool call it ran, and how
/// many model calls it took.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnSummary {
    /// The text of every model reply in the turn, joined.
    pub response: String,
    /// Every tool call the turn ran, in call order.
    pub tool_calls: Vec<ToolCall>,
    /// How many model calls the turn made.
    pub iterations: usize,
}

/// One tool call of a turn: the tool, the model's input and what the model
/// read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The name of the tool called.
    pub name: String,
    /// The input the model wrote.
    pub input: serde_json::Value,
    /// The result the model read, capped at `max_tool_output_chars`.
    pub output: ToolOutput,
}

/// An agent: a model, the tools it may call, and the conversation so
/// far. Each [`run_turn`](Self::run_turn) adds the prompt, the replies and
/// the tool results to the history.
///
/// ```no_run
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::agent::{ConversationRuntime, Tools};
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::fs_tools::{Files, Sandbox};
/// # #[cfg(feature = "fs-tools")]
/// use llm_provider::OpenAi;
///
/// # #[cfg(feature = "fs-tools")]
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let model = OpenAi::at("http://localhost:11434/v1").llm("qwen3");
///
///     let sandbox = Sandbox::new("docs")?;
///     let tools = Tools::new()
///         .with_all(Files::new(sandbox))
///         .only(["read", "edit"]);
///
///     let mut agent = ConversationRuntime::builder(model, tools)
///         .system("You fix typos in the files you are given.")
///         .max_iterations(10)
///         .on_delta(|text| print!("{text}"))
///         .build();
///
///     let turn = agent.run_turn("Fix the typos in README.md.").await?;
///     eprintln!("{} tool calls", turn.tool_calls.len());
///     Ok(())
/// }
/// # #[cfg(not(feature = "fs-tools"))]
/// # fn main() {}
/// ```
pub struct ConversationRuntime<T> {
    llm: Llm,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
    messages: Vec<Message>,
}

impl<T: Toolbox> ConversationRuntime<T> {
    /// A builder for a runtime on `llm` that offers the tools in
    /// `toolbox`.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1").llm("qwen3");
    /// let agent = ConversationRuntime::builder(model, Tools::new())
    ///     .system("You answer in one sentence.")
    ///     .max_tokens(1024)
    ///     .build();
    /// # let _ = agent;
    /// ```
    pub fn builder(llm: Llm, toolbox: T) -> RuntimeBuilder<T> {
        RuntimeBuilder::new(llm, toolbox)
    }

    /// A runtime with no system prompt, `max_tokens` 8192, 25 model calls
    /// per turn, 4 tool calls at once and tool results capped at 100 000
    /// characters.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1").llm("qwen3");
    /// let agent = ConversationRuntime::new(model, Tools::new());
    /// # let _ = agent;
    /// ```
    pub fn new(llm: Llm, toolbox: T) -> Self {
        Self::builder(llm, toolbox).build()
    }

    /// The runtime [`RuntimeBuilder::build`] assembles.
    pub(super) fn from_parts(
        llm: Llm,
        toolbox: T,
        settings: Settings,
        on_delta: Option<OnDelta>,
    ) -> Self {
        Self {
            llm,
            toolbox,
            settings,
            on_delta,
            messages: Vec::new(),
        }
    }

    /// The conversation so far, oldest first: every prompt, reply, tool
    /// call and tool result of the turns run.
    #[must_use]
    pub fn history(&self) -> &[Message] {
        &self.messages
    }

    /// This agent as a [`Tool`](crate::agent::Tool) called `name` that
    /// another agent calls with `{"prompt": "…"}`; each call runs one
    /// [`run_turn`](Self::run_turn) and answers with its response. The
    /// agent keeps its conversation across calls, and calls to it run one
    /// at a time.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let worker = OpenAi::at("http://localhost:11434/v1").llm("qwen3");
    /// let worker = ConversationRuntime::new(worker, Tools::new())
    ///     .into_tool("worker", "Answers one question. Give it the question.");
    /// let lead = OpenAi::at("http://localhost:11434/v1").llm("qwen3");
    /// let lead = ConversationRuntime::new(lead, Tools::new().with(worker));
    /// # let _ = lead;
    /// ```
    pub fn into_tool(
        self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> AgentTool<T> {
        AgentTool::new(self, name.into(), description.into())
    }

    /// Runs one turn: sends `prompt`, runs the tools the model asks for,
    /// feeds their results back, and repeats until the model answers
    /// without a tool call.
    ///
    /// # Errors
    /// [`RuntimeError::Model`] when a model call fails,
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
                if !reply.text.is_empty() {
                    self.messages.push(Message::assistant(reply.text));
                }
                return Ok(turn);
            }

            let calls = self.run_tools(reply.text, reply.tool_uses).await;
            turn.tool_calls.extend(calls);
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
            response_schema: None,
        };

        let on_delta = &mut self.on_delta;
        fold(self.llm.stream(request), |text| {
            if let Some(on_delta) = on_delta.as_mut() {
                on_delta(text);
            }
        })
        .await
    }

    /// Runs `calls`, at most `tool_concurrency` at once, each reading the
    /// conversation as it stands; appends the reply's `text`, the calls and
    /// their results, in call order, to the history and returns their records.
    async fn run_tools(&mut self, text: String, calls: Vec<ToolUseCall>) -> Vec<ToolCall> {
        let cap = self.settings.max_tool_output_chars;
        let definitions = self.toolbox.definitions();
        let transcript = Transcript {
            system: self.settings.system.as_deref(),
            tools: &definitions,
            messages: &self.messages,
        };

        let runs: Vec<_> = calls
            .iter()
            .map(|call| call_tool(&self.toolbox, call, &transcript, cap))
            .collect();
        let outputs: Vec<ToolOutput> = stream::iter(runs)
            .buffered(self.settings.tool_concurrency.max(1))
            .collect()
            .await;

        self.record(text, calls, outputs)
    }

    /// Appends the reply's `text` and `calls` to the history as one assistant
    /// message (text first, left out when empty) and their `outputs` as one
    /// user message, and returns them as records.
    fn record(
        &mut self,
        text: String,
        calls: Vec<ToolUseCall>,
        outputs: Vec<ToolOutput>,
    ) -> Vec<ToolCall> {
        let lead = (!text.is_empty()).then_some(Content::Text { text });
        let uses = calls.iter().map(|call| Content::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input: call.input.clone(),
        });
        let results = calls
            .iter()
            .zip(&outputs)
            .map(|(call, output)| Content::ToolResult {
                tool_use_id: call.id.clone(),
                content: output.content.clone(),
                is_error: output.is_error,
            });
        self.messages.push(Message {
            role: MessageRole::Assistant,
            content: lead.into_iter().chain(uses).collect(),
        });
        self.messages.push(Message {
            role: MessageRole::User,
            content: results.collect(),
        });

        let records = calls.into_iter().zip(outputs);
        records
            .map(|(call, output)| ToolCall {
                name: call.name,
                input: call.input,
                output,
            })
            .collect()
    }
}

/// Runs one tool call through `toolbox` and turns its output, or its
/// panic, into a result capped at `cap` characters.
async fn call_tool<T: Toolbox>(
    toolbox: &T,
    call: &ToolUseCall,
    transcript: &Transcript<'_>,
    cap: usize,
) -> ToolOutput {
    let run = toolbox.call(&call.name, call.input.clone(), transcript);
    let output = AssertUnwindSafe(run).catch_unwind().await;
    let output = output.unwrap_or_else(|_| ToolOutput {
        content: format!("tool {} panicked", call.name),
        is_error: true,
    });
    cap_output(output, cap)
}

// Hand-written: the toolbox and the callback need not be `Debug`.
impl<T> fmt::Debug for ConversationRuntime<T> {
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
