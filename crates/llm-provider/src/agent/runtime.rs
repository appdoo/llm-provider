//! [`ConversationRuntime`]: the turn loop over one model and one
//! [`Toolbox`].

use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures::{stream, FutureExt, StreamExt};
use tokio_util::sync::CancellationToken;

use crate::agent::agent_tool::AgentTool;
use crate::agent::builder::{OnDelta, RuntimeBuilder, Settings};
use crate::agent::context::CallContext;
use crate::agent::error::RuntimeError;
use crate::agent::tool::{ToolOutput, Toolbox, Transcript};
use crate::provider::{
    fold_cancellable, Completion, CompletionRequest, Content, Error, Llm, LlmProvider, Message,
    MessageRole, ModelSelection, Sampling, StopReason, ToolUseCall, Usage,
};

/// What one turn did: the model's text, every tool call it ran, how many
/// model calls it took, and what they cost in tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TurnSummary {
    /// The text of every model reply in the turn, joined.
    pub response: String,
    /// Every tool call the turn ran, in call order.
    pub tool_calls: Vec<ToolCall>,
    /// How many model calls the turn made.
    pub iterations: usize,
    /// The token counts of the turn's model calls, added up; each call's
    /// counts are its own, never a running total.
    pub usage: Usage,
}

/// One tool call of a turn: the tool, the model's input and what the model
/// read back.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
///     let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
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
    turns: u64,
}

impl<T: Toolbox> ConversationRuntime<T> {
    /// A builder for a runtime on `llm` that offers the tools in
    /// `toolbox`.
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let agent = ConversationRuntime::builder(model, Tools::new())
    ///     .system("You answer in one sentence.")
    ///     .max_tokens(1024)
    ///     .build();
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
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
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let agent = ConversationRuntime::new(model, Tools::new());
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
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
            turns: 0,
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
    /// let worker = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let worker = ConversationRuntime::new(worker, Tools::new())
    ///     .into_tool("worker", "Answers one question. Give it the question.");
    /// let lead = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let lead = ConversationRuntime::new(lead, Tools::new().with(worker));
    /// # let _ = lead;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
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
    /// It is [`run_turn_cancellable`](Self::run_turn_cancellable) with a
    /// token that never fires. A dropped `run_turn` future leaves the
    /// history as it was before the call.
    ///
    /// # Errors
    /// [`RuntimeError::Model`] when a model call fails,
    /// [`RuntimeError::Truncated`] when a reply stops at `max_tokens`, and
    /// [`RuntimeError::MaxIterations`] when the model still asks for tools
    /// after the last allowed call. Only a successful turn changes the
    /// history; each of these errors carries the turn so far
    /// ([`RuntimeError::turn`]), so a caller can log, bill or retry with it.
    #[allow(clippy::result_large_err)] // the error carries the turn so far; boxing it would hide it from callers
    pub async fn run_turn(&mut self, prompt: &str) -> Result<TurnSummary, RuntimeError> {
        self.run_turn_cancellable(prompt, &CancellationToken::new())
            .await
    }

    /// Runs one turn as [`run_turn`](Self::run_turn) does, until `token`
    /// fires. On cancel it drops the model stream and the running tool
    /// calls, writes nothing more to the history, and rolls the history back
    /// to before the call; a dropped future rolls it back as well. Tools see
    /// `token` as [`CallContext::cancel`].
    ///
    /// ```no_run
    /// use llm_provider::agent::{ConversationRuntime, RuntimeError, Tools};
    /// use llm_provider::OpenAi;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let model = OpenAi::at("http://localhost:8080/v1").expect("a valid URL").llm("qwen3-8b");
    ///     let mut agent = ConversationRuntime::builder(model, Tools::new()).build();
    ///     let token = CancellationToken::new();
    ///
    ///     match agent.run_turn_cancellable("Plan the refactor.", &token).await {
    ///         Err(RuntimeError::Interrupted { partial_text, tool_calls, .. }) => {
    ///             assert!(agent.history().is_empty()); // no dangling user message
    ///             println!("cut after {} tool calls: {partial_text}", tool_calls.len());
    ///         }
    ///         other => println!("{other:?}"),
    ///     }
    /// }
    /// ```
    ///
    /// # Errors
    /// [`RuntimeError::Interrupted`] when `token` fires, with the turn's
    /// text and the tool calls that completed; otherwise as
    /// [`run_turn`](Self::run_turn); on every error the history stays as it was
    /// before the call.
    #[allow(clippy::result_large_err)] // the error carries the turn so far; boxing it would hide it from callers
    pub async fn run_turn_cancellable(
        &mut self,
        prompt: &str,
        token: &CancellationToken,
    ) -> Result<TurnSummary, RuntimeError> {
        self.turns += 1;

        // The turn works on a copy; only a successful turn reaches the store.
        let mut messages = self.messages.clone();
        let result = self.turn_loop(&mut messages, prompt, token).await;
        if result.is_ok() {
            self.messages = messages;
        }
        result
    }

    /// The turn itself: appends `prompt`, then alternates model calls and
    /// tool calls until the model answers without one, or `token` fires.
    #[allow(clippy::result_large_err)] // the error carries the turn so far; boxing it would hide it from callers
    async fn turn_loop(
        &mut self,
        messages: &mut Vec<Message>,
        prompt: &str,
        token: &CancellationToken,
    ) -> Result<TurnSummary, RuntimeError> {
        messages.push(Message::user(prompt));
        let mut turn = TurnSummary::default();

        loop {
            if turn.iterations == self.settings.max_iterations {
                return Err(RuntimeError::MaxIterations {
                    limit: self.settings.max_iterations,
                    turn,
                });
            }

            let reply = match self.next_reply(messages, token).await {
                Ok(reply) => reply,
                Err(error) => return Err(model_error(error, turn)),
            };
            turn.iterations += 1;
            turn.response.push_str(&reply.text);
            turn.usage += reply.usage;
            if reply.stop == StopReason::MaxTokens {
                return Err(RuntimeError::Truncated { turn });
            }

            if reply.tool_uses.is_empty() {
                if !reply.text.is_empty() {
                    messages.push(Message::assistant(reply.text));
                }
                return Ok(turn);
            }

            match self
                .run_tools(messages, reply.text, reply.tool_uses, token)
                .await
            {
                Ok(calls) => turn.tool_calls.extend(calls),
                Err(done) => return Err(interrupted(turn, "", done)),
            }
        }
    }

    /// Sends the history to the model and folds its reply, handing each
    /// text delta to the `on_delta` receiver.
    async fn next_reply(
        &mut self,
        messages: &[Message],
        token: &CancellationToken,
    ) -> Result<Completion, Error> {
        let mut request = CompletionRequest {
            model: ModelSelection::Default,
            system: self.settings.system.clone(),
            messages: Arc::from(messages),
            tools: self.toolbox.definitions(),
            max_tokens: self.settings.max_tokens,
            cache_breakpoints: Vec::new(),
            response_schema: None,
            sampling: Sampling::default(),
        };
        if let Some(ttl) = self.settings.cache {
            request = request.cache(ttl);
        }

        let on_delta = &mut self.on_delta;
        let on_delta = |text: &str| {
            if let Some(on_delta) = on_delta.as_mut() {
                on_delta(text);
            }
        };
        fold_cancellable(self.llm.stream(request), on_delta, token).await
    }

    /// Runs `calls`, at most `tool_concurrency` at once, each reading the
    /// conversation as it stands; appends the reply's `text`, the calls and
    /// their results, in call order, to the history and returns their records.
    /// When `token` fires it drops the running calls, appends nothing and
    /// returns the records of the calls that completed in order as `Err`.
    async fn run_tools(
        &mut self,
        messages: &mut Vec<Message>,
        text: String,
        calls: Vec<ToolUseCall>,
        token: &CancellationToken,
    ) -> Result<Vec<ToolCall>, Vec<ToolCall>> {
        let cap = self.settings.max_tool_output_chars;
        let timeout = self.settings.tool_timeout;
        let definitions = self.toolbox.definitions();
        let context = CallContext {
            transcript: Transcript {
                system: self.settings.system.as_deref(),
                tools: &definitions,
                messages,
            },
            cancel: token.clone(),
            turn: self.turns,
        };

        let runs: Vec<_> = calls
            .iter()
            .map(|call| call_tool(&self.toolbox, call, &context, cap, timeout))
            .collect();
        let concurrency = self.settings.tool_concurrency.max(1);
        let outputs = match outputs_until(runs, concurrency, token).await {
            Ok(outputs) => outputs,
            Err(done) => return Err(records(calls, done)),
        };

        Ok(Self::record(messages, text, calls, outputs))
    }

    /// Appends the reply's `text` and `calls` to the `messages` as one assistant
    /// message (text first, left out when empty) and their `outputs` as one
    /// user message, and returns them as records.
    fn record(
        messages: &mut Vec<Message>,
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
        messages.push(Message {
            role: MessageRole::Assistant,
            content: lead.into_iter().chain(uses).collect(),
        });
        messages.push(Message {
            role: MessageRole::User,
            content: results.collect(),
        });

        records(calls, outputs)
    }
}

/// The error of a turn whose model call failed with `error`: a cancel
/// becomes [`RuntimeError::Interrupted`], anything else
/// [`RuntimeError::Model`].
fn model_error(error: Error, turn: TurnSummary) -> RuntimeError {
    match error {
        Error::Interrupted { partial_text, .. } => interrupted(turn, &partial_text, Vec::new()),
        error => RuntimeError::Model { error, turn },
    }
}

/// The error of a turn cut by its token: the turn's text with the cut
/// reply's `partial_text` added, and its completed tool calls followed by
/// `done`.
fn interrupted(turn: TurnSummary, partial_text: &str, done: Vec<ToolCall>) -> RuntimeError {
    let mut tool_calls = turn.tool_calls;
    tool_calls.extend(done);
    RuntimeError::Interrupted {
        partial_text: turn.response + partial_text,
        tool_calls,
    }
}

/// The outputs of `runs`, at most `concurrency` at once, in call order; when
/// `token` fires first, the outputs already yielded as `Err`.
async fn outputs_until<F>(
    runs: Vec<F>,
    concurrency: usize,
    token: &CancellationToken,
) -> Result<Vec<ToolOutput>, Vec<ToolOutput>>
where
    F: Future<Output = ToolOutput>,
{
    let mut pending = stream::iter(runs).buffered(concurrency);
    let mut outputs = Vec::new();

    loop {
        tokio::select! {
            biased;
            next = pending.next() => match next {
                Some(output) => outputs.push(output),
                None => return Ok(outputs),
            },
            () = token.cancelled() => return Err(outputs),
        }
    }
}

/// The records of `calls` paired with their `outputs`, in call order; calls
/// without an output are left out.
fn records(calls: Vec<ToolUseCall>, outputs: Vec<ToolOutput>) -> Vec<ToolCall> {
    calls
        .into_iter()
        .zip(outputs)
        .map(|(call, output)| ToolCall {
            name: call.name,
            input: call.input,
            output,
        })
        .collect()
}

/// Runs one tool call through `toolbox` and turns its output, its panic or
/// its running past `timeout` into a result capped at `cap` characters.
async fn call_tool<T: Toolbox>(
    toolbox: &T,
    call: &ToolUseCall,
    context: &CallContext<'_>,
    cap: usize,
    timeout: Duration,
) -> ToolOutput {
    let run = toolbox.call(&call.name, call.input.clone(), context);
    let output = tokio::time::timeout(timeout, AssertUnwindSafe(run).catch_unwind()).await;
    let output = match output {
        Ok(Ok(output)) => output,
        Ok(Err(_)) => ToolOutput {
            content: format!("tool {} panicked", call.name),
            is_error: true,
        },
        Err(_) => ToolOutput {
            content: format!("tool {} timed out after {} s", call.name, timeout.as_secs()),
            is_error: true,
        },
    };
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
