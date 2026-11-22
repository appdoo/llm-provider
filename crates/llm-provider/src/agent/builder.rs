//! [`SessionBuilder`]: the settings of a
//! [`Session`] before it runs.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::agent::compaction::{CompactError, Compaction};
use crate::agent::event::SessionEvent;
use crate::agent::runtime::Session;
use crate::agent::tool::Toolbox;
use crate::provider::{CacheTtl, Llm, Message};

/// The receiver of streamed text, as [`SessionBuilder::on_delta`] stores it.
pub(super) type OnDelta = Box<dyn FnMut(&str) + Send>;

/// The tunable limits of a turn, with the defaults of
/// [`Session::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Settings {
    pub(super) system: Option<String>,
    pub(super) max_tokens: u32,
    pub(super) max_iterations: usize,
    pub(super) tool_concurrency: usize,
    pub(super) max_tool_output_chars: usize,
    pub(super) cache: Option<CacheTtl>,
    pub(super) tool_timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            system: None,
            max_tokens: 8192,
            max_iterations: 25,
            tool_concurrency: 4,
            max_tool_output_chars: 100_000,
            cache: Some(CacheTtl::FiveMinutes),
            tool_timeout: Duration::from_secs(120),
        }
    }
}

/// Where a session starts: a fresh one, or the state folded from the events
/// [`SessionBuilder::restore`] takes.
pub(super) struct Origin {
    pub(super) session_id: Uuid,
    pub(super) messages: Vec<Message>,
    pub(super) next_seq: u64,
    pub(super) turns: u64,
    pub(super) last_input: u64,
}

impl Origin {
    /// A new session: a fresh version 7 id, no history, seq 0.
    pub(super) fn fresh() -> Self {
        Self {
            session_id: Uuid::now_v7(),
            messages: Vec::new(),
            next_seq: 0,
            turns: 0,
            last_input: 0,
        }
    }
}

/// Settings for a [`Session`]: its system prompt, its limits
/// and the receiver of streamed text. Unset values keep the defaults of
/// [`Session::new`].
#[must_use]
pub struct SessionBuilder<T> {
    llm: Llm,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
    events: Option<Sender<SessionEvent>>,
    origin: Option<Origin>,
    compaction: Option<Compaction>,
}

impl<T: Toolbox> SessionBuilder<T> {
    /// A builder for a runtime on `llm` that offers `toolbox`.
    pub(super) fn new(llm: Llm, toolbox: T) -> Self {
        Self {
            llm,
            toolbox,
            settings: Settings::default(),
            on_delta: None,
            events: None,
            origin: None,
            compaction: None,
        }
    }

    /// Sets the system prompt sent with every request.
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.settings.system = Some(system.into());
        self
    }

    /// Sets how long the prompt prefix is cached (default five minutes).
    /// Every model call marks the tools, the system prompt and the last
    /// message, so the cache grows with the history; below the model's
    /// minimum prompt size nothing is cached and no error comes. OpenAI
    /// ignores it.
    ///
    /// Cost: a cache write costs 1.25 times the input price (2 times for
    /// [`CacheTtl::OneHour`]), a read 0.1 times. The five-minute default
    /// pays within a tool loop, where calls come seconds apart; a chat with
    /// more than the lifetime between turns writes every time and reads
    /// nothing, so use [`CacheTtl::OneHour`] or [`no_cache`](Self::no_cache)
    /// there.
    pub fn cache(mut self, ttl: CacheTtl) -> Self {
        self.settings.cache = Some(ttl);
        self
    }

    /// Turns prompt caching off; no request carries a cache marker.
    pub fn no_cache(mut self) -> Self {
        self.settings.cache = None;
        self
    }

    /// Sets the `max_tokens` of each request (default 8192).
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.settings.max_tokens = max_tokens;
        self
    }

    /// Sets how many model calls one turn may make (default 25); one more
    /// is [`SessionError::MaxIterations`](crate::agent::SessionError::MaxIterations).
    pub fn max_iterations(mut self, max_iterations: usize) -> Self {
        self.settings.max_iterations = max_iterations;
        self
    }

    /// Sets how many tool calls run at once (default 4).
    pub fn tool_concurrency(mut self, tool_concurrency: usize) -> Self {
        self.settings.tool_concurrency = tool_concurrency;
        self
    }

    /// Sets the longest tool result in characters (default 100 000);
    /// longer output keeps its head and tail.
    pub fn max_tool_output_chars(mut self, max_tool_output_chars: usize) -> Self {
        self.settings.max_tool_output_chars = max_tool_output_chars;
        self
    }

    /// Sets how long one tool call may run (default 120 seconds). A call
    /// still running then is dropped and the model reads an error result
    /// saying it timed out; the turn goes on.
    pub fn tool_timeout(mut self, tool_timeout: Duration) -> Self {
        self.settings.tool_timeout = tool_timeout;
        self
    }

    /// Hands each piece of streamed text to `on_delta` as it arrives.
    pub fn on_delta(mut self, on_delta: impl FnMut(&str) + Send + 'static) -> Self {
        self.on_delta = Some(Box::new(on_delta));
        self
    }

    /// Sends a [`SessionEvent`] on `events` after every model call. The
    /// session awaits each `send`, so a full channel slows it down, and a
    /// closed channel ends the turn with [`SessionError::Store`](crate::agent::SessionError::Store). Without
    /// this nothing is sent.
    ///
    /// ```no_run
    /// use llm_provider::agent::{Session, SessionEvent, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let (tx, mut rx) = tokio::sync::mpsc::channel::<SessionEvent>(64);
    /// tokio::spawn(async move {
    ///     while let Some(event) = rx.recv().await {
    ///         println!("{}", serde_json::to_string(&event).unwrap_or_default());
    ///     }
    /// });
    /// let agent = Session::builder(model, Tools::new()).events(tx).build();
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    pub fn events(mut self, events: Sender<SessionEvent>) -> Self {
        self.events = Some(events);
        self
    }

    /// Continues a stored session from its `events`: all events of one
    /// session, or those from its last [`SessionEvent::Compacted`] on, in
    /// any order (the crate sorts by [`seq`](SessionEvent::seq)).
    ///
    /// The session starts with the history the events add up to: the last
    /// compacted history, then the messages of the model calls of every turn
    /// that ended ok. A turn without an ok [`SessionEvent::TurnEnded`] did
    /// not join the history. Its id is the stored one, its next event
    /// numbering follows the highest seq, its next turn the highest turn
    /// plus one, and the input of the last model call decides whether the
    /// first turn compacts. No events give a fresh session.
    ///
    /// Events of two session ids: the id of the highest-seq event wins and
    /// the other events are ignored, so [`build`](Self::build) stays
    /// infallible. The system prompt and tools come from the app, set on
    /// this builder as for a new session.
    ///
    /// ```no_run
    /// use llm_provider::agent::{Session, SessionEvent, Tools};
    /// use llm_provider::OpenAi;
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let events: Vec<SessionEvent> = Vec::new(); // read from your store
    /// let agent = Session::builder(model, Tools::new()).restore(events).build();
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    pub fn restore(mut self, events: impl IntoIterator<Item = SessionEvent>) -> Self {
        self.origin = Some(Origin::from_events(events));
        self
    }

    /// Compacts the history when it grows long. At the start of a turn,
    /// when the last model call read `threshold_input_tokens` or more input
    /// tokens (cache reads and writes included), the session passes its
    /// history to `callback` and continues with the messages it returns;
    /// it never compacts inside a turn's tool loop. A compaction stays when
    /// its turn then fails. A
    /// [`SessionEvent::Compacted`] records the new history. A callback error
    /// ends the turn as [`SessionError::Compact`](crate::agent::SessionError::Compact)
    /// with the history unchanged. The crate ships no summarizer.
    ///
    /// The threshold is an absolute token count the app chooses; the crate
    /// knows no model's context window. It is compared with the last call's
    /// input, which does not count that call's reply or the new prompt, so
    /// leave headroom below the window. An endpoint that reports no usage
    /// never reaches the threshold.
    ///
    /// ```no_run
    /// use llm_provider::agent::{Session, Tools};
    /// use llm_provider::{Message, OpenAi};
    ///
    /// let model = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3");
    /// let agent = Session::builder(model, Tools::new())
    ///     .compact_when(150_000, |history: Vec<Message>| async move {
    ///         let note = format!("{} earlier messages were dropped.", history.len());
    ///         Ok(vec![Message::user(note)])
    ///     })
    ///     .build();
    /// # let _ = agent;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    pub fn compact_when<F, Fut>(mut self, threshold_input_tokens: u64, callback: F) -> Self
    where
        F: FnMut(Vec<Message>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<Vec<Message>, CompactError>> + Send + 'static,
    {
        self.compaction = Some(Compaction::new(threshold_input_tokens, callback));
        self
    }

    /// A runtime with these settings, and an empty history with a fresh
    /// session id unless [`restore`](Self::restore) was called.
    pub fn build(self) -> Session<T> {
        let origin = self.origin.unwrap_or_else(Origin::fresh);
        Session::from_parts(
            self.llm,
            self.toolbox,
            self.settings,
            self.on_delta,
            self.events,
            self.compaction,
            origin,
        )
    }
}

// Hand-written: the toolbox and the callback need not be `Debug`.
impl<T> fmt::Debug for SessionBuilder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
