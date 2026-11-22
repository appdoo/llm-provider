//! [`Session`]: the turn loop over one model and one
//! [`Toolbox`].

use std::fmt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures::{stream, FutureExt, StreamExt};
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agent::agent_tool::AgentTool;
use crate::agent::builder::{OnDelta, Origin, SessionBuilder, Settings};
use crate::agent::compaction::Compaction;
use crate::agent::context::CallContext;
use crate::agent::error::SessionError;
use crate::agent::event::SessionEvent;
use crate::agent::tool::{ToolOutput, Toolbox, Tools, Transcript};
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
/// use llm_provider::agent::{Session, Tools};
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
///     let mut agent = Session::builder(model, tools)
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
pub struct Session<T = Tools> {
    llm: Llm,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
    events: Option<Sender<SessionEvent>>,
    setup_sent: bool,
    pub(super) compaction: Option<Compaction>,
    id: Uuid,
    next_seq: u64,
    pub(super) messages: Vec<Message>,
    turns: u64,
}

impl<T: Toolbox> Session<T> {
    /// A builder for a runtime on `llm` that offers the tools in
    /// `toolbox`.
    ///
    /// ```no_run
    /// use llm_provider::agent::{Session, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let agent = Session::builder(model, Tools::new())
    ///     .system("You answer in one sentence.")
    ///     .max_tokens(1024)
    ///     .build();
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    pub fn builder(llm: Llm, toolbox: T) -> SessionBuilder<T> {
        SessionBuilder::new(llm, toolbox)
    }

    /// A runtime with no system prompt, `max_tokens` 8192, 25 model calls
    /// per turn, 4 tool calls at once and tool results capped at 100 000
    /// characters.
    ///
    /// ```no_run
    /// use llm_provider::agent::{Session, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let agent = Session::new(model, Tools::new());
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    pub fn new(llm: Llm, toolbox: T) -> Self {
        Self::builder(llm, toolbox).build()
    }

    /// The runtime [`SessionBuilder::build`] assembles.
    pub(super) fn from_parts(
        llm: Llm,
        toolbox: T,
        settings: Settings,
        on_delta: Option<OnDelta>,
        events: Option<Sender<SessionEvent>>,
        compaction: Option<Compaction>,
        origin: Origin,
    ) -> Self {
        let mut compaction = compaction;
        if let Some(compaction) = &mut compaction {
            compaction.seed(origin.last_input);
        }
        Self {
            llm,
            toolbox,
            settings,
            on_delta,
            events,
            setup_sent: false,
            compaction,
            id: origin.session_id,
            next_seq: origin.next_seq,
            messages: origin.messages,
            turns: origin.turns,
        }
    }

    /// The id of this session: a version 7 UUID made at
    /// [`build`](SessionBuilder::build), or the one
    /// [`restore`](SessionBuilder::restore) named. Every
    /// [`SessionEvent`] carries it.
    #[must_use]
    pub fn session_id(&self) -> Uuid {
        self.id
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
    /// use llm_provider::agent::{Session, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let worker = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let worker = Session::new(worker, Tools::new())
    ///     .into_tool("worker", "Answers one question. Give it the question.");
    /// let lead = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let lead = Session::new(lead, Tools::new().with(worker));
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
    /// [`SessionError::Model`] when a model call fails,
    /// [`SessionError::Truncated`] when a reply stops at `max_tokens`, and
    /// [`SessionError::MaxIterations`] when the model still asks for tools
    /// after the last allowed call. Only a successful turn adds to the
    /// history (a compaction at the start of the turn stays even when the
    /// turn fails); each of these errors carries the turn so far
    /// ([`SessionError::turn`]), so a caller can log, bill or retry with it.
    pub async fn run_turn(&mut self, prompt: &str) -> Result<TurnSummary, SessionError> {
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
    /// use llm_provider::agent::{Session, SessionError, Tools};
    /// use llm_provider::OpenAi;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let model = OpenAi::at("http://localhost:8080/v1").expect("a valid URL").llm("qwen3-8b");
    ///     let mut agent = Session::builder(model, Tools::new()).build();
    ///     let token = CancellationToken::new();
    ///
    ///     match agent.run_turn_cancellable("Plan the refactor.", &token).await {
    ///         Err(SessionError::Interrupted { turn, .. }) => {
    ///             assert!(agent.history().is_empty()); // no dangling user message
    ///             println!("cut after {} tool calls: {}", turn.tool_calls.len(), turn.response);
    ///         }
    ///         other => println!("{other:?}"),
    ///     }
    /// }
    /// ```
    ///
    /// # Errors
    /// [`SessionError::Interrupted`] when `token` fires, with the turn's
    /// text and the tool calls that completed; otherwise as
    /// [`run_turn`](Self::run_turn); on every error the history stays as it was
    /// before the call, except for a compaction at the start of the turn.
    pub async fn run_turn_cancellable(
        &mut self,
        prompt: &str,
        token: &CancellationToken,
    ) -> Result<TurnSummary, SessionError> {
        self.turns += 1;

        // Compaction stays even when the turn then fails.
        let compacted = self.compact_if_due().await;

        // The turn works on a copy; only a successful turn reaches the store.
        let mut messages = self.messages.clone();
        let result = match compacted {
            Ok(()) => self.turn_loop(&mut messages, prompt, token).await,
            Err(error) => Err(error),
        };
        let result = self.emit_turn_ended(result).await;
        if result.is_ok() {
            self.messages = messages;
        }
        result
    }

    /// The turn itself: appends `prompt`, then alternates model calls and
    /// tool calls until the model answers without one, or `token` fires.
    async fn turn_loop(
        &mut self,
        messages: &mut Vec<Message>,
        prompt: &str,
        token: &CancellationToken,
    ) -> Result<TurnSummary, SessionError> {
        let mut sent = messages.len();
        messages.push(Message::user(prompt));
        let mut turn = TurnSummary::default();

        loop {
            if turn.iterations == self.settings.max_iterations {
                return Err(SessionError::MaxIterations {
                    limit: self.settings.max_iterations,
                    turn,
                });
            }

            let started = (SystemTime::now(), Instant::now());
            let iteration = turn.iterations + 1;
            let result = self.next_reply(messages, token).await;

            // Count the paid call and keep its message before the event goes out.
            if let Ok(reply) = &result {
                turn.iterations += 1;
                turn.response.push_str(&reply.text);
                turn.usage += reply.usage;
                self.note_input(reply.usage);
                messages.extend(assistant_message(reply));
            }
            let emitted = self
                .emit_call(
                    messages,
                    &mut sent,
                    iteration,
                    &turn,
                    started,
                    result.as_ref(),
                )
                .await;
            let reply = match result {
                Ok(reply) => {
                    emitted?;
                    reply
                }
                // A failed call stays a `Model` error even if its event was lost.
                Err(error) => return Err(model_error(error, turn)),
            };
            if reply.stop == StopReason::MaxTokens {
                return Err(SessionError::Truncated { turn });
            }
            if reply.tool_uses.is_empty() {
                return Ok(turn);
            }

            match self.run_tools(messages, &reply.tool_uses, token).await {
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
    /// conversation as it stands; appends their results, in call order, as one
    /// user message to the history and returns their records. When `token`
    /// fires it drops the running calls, appends nothing and returns the
    /// records of the calls that completed in order as `Err`.
    async fn run_tools(
        &mut self,
        messages: &mut Vec<Message>,
        calls: &[ToolUseCall],
        token: &CancellationToken,
    ) -> Result<Vec<ToolCall>, Vec<ToolCall>> {
        let cap = self.settings.max_tool_output_chars;
        let timeout = self.settings.tool_timeout;
        let definitions = self.toolbox.definitions();

        // The calls read the conversation without the message that asks for them.
        let asked = messages.len().saturating_sub(1);
        let context = CallContext {
            transcript: Transcript {
                system: self.settings.system.as_deref(),
                tools: &definitions,
                messages: &messages[..asked],
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

        Ok(Self::record(messages, calls, outputs))
    }

    /// Appends the `outputs` of `calls` to the `messages` as one user
    /// message and returns them as records.
    fn record(
        messages: &mut Vec<Message>,
        calls: &[ToolUseCall],
        outputs: Vec<ToolOutput>,
    ) -> Vec<ToolCall> {
        let results = calls
            .iter()
            .zip(&outputs)
            .map(|(call, output)| Content::ToolResult {
                tool_use_id: call.id.clone(),
                content: output.content.clone(),
                is_error: output.is_error,
            });
        messages.push(Message {
            role: MessageRole::User,
            content: results.collect(),
        });

        records(calls, outputs)
    }
}

/// The assistant message `reply` adds to the history: its text, then its
/// tool calls; `None` when it has neither.
fn assistant_message(reply: &Completion) -> Option<Message> {
    let lead = (!reply.text.is_empty()).then(|| Content::Text {
        text: reply.text.clone(),
    });
    let uses = reply.tool_uses.iter().map(|call| Content::ToolUse {
        id: call.id.clone(),
        name: call.name.clone(),
        input: call.input.clone(),
    });
    let content: Vec<Content> = lead.into_iter().chain(uses).collect();
    (!content.is_empty()).then(|| Message::new(MessageRole::Assistant, content))
}

/// The error of a turn whose model call failed with `error`: a cancel
/// becomes [`SessionError::Interrupted`], anything else
/// [`SessionError::Model`].
fn model_error(error: Error, turn: TurnSummary) -> SessionError {
    match error {
        Error::Interrupted { partial_text, .. } => interrupted(turn, &partial_text, Vec::new()),
        error => SessionError::Model { error, turn },
    }
}

/// The error of a turn cut by its token: the turn's text with the cut
/// reply's `partial_text` added, and its completed tool calls followed by
/// `done`.
fn interrupted(mut turn: TurnSummary, partial_text: &str, done: Vec<ToolCall>) -> SessionError {
    turn.response.push_str(partial_text);
    turn.tool_calls.extend(done);
    SessionError::Interrupted { turn }
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
fn records(calls: &[ToolUseCall], outputs: Vec<ToolOutput>) -> Vec<ToolCall> {
    calls
        .iter()
        .zip(outputs)
        .map(|(call, output)| ToolCall {
            name: call.name.clone(),
            input: call.input.clone(),
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
impl<T> fmt::Debug for Session<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("settings", &self.settings)
            .field("messages", &self.messages.len())
            .field("next_seq", &self.next_seq)
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

mod events;
mod restore;

#[cfg(test)]
mod tests;
