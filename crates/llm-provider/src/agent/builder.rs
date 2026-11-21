//! [`SessionBuilder`]: the settings of a
//! [`Session`] before it runs.

use std::fmt;
use std::time::Duration;

use crate::agent::runtime::Session;
use crate::agent::tool::Toolbox;
use crate::provider::{CacheTtl, Llm};

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

/// Settings for a [`Session`]: its system prompt, its limits
/// and the receiver of streamed text. Unset values keep the defaults of
/// [`Session::new`].
#[must_use]
pub struct SessionBuilder<T> {
    llm: Llm,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
}

impl<T: Toolbox> SessionBuilder<T> {
    /// A builder for a runtime on `llm` that offers `toolbox`.
    pub(super) fn new(llm: Llm, toolbox: T) -> Self {
        Self {
            llm,
            toolbox,
            settings: Settings::default(),
            on_delta: None,
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

    /// A runtime with these settings and an empty history.
    pub fn build(self) -> Session<T> {
        Session::from_parts(self.llm, self.toolbox, self.settings, self.on_delta)
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
