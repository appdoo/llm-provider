//! [`RuntimeBuilder`]: the settings of a
//! [`ConversationRuntime`] before it runs.

use std::fmt;

use crate::agent::runtime::ConversationRuntime;
use crate::agent::tool::Toolbox;
use crate::provider::LlmProvider;

/// The receiver of streamed text, as [`RuntimeBuilder::on_delta`] stores it.
pub(super) type OnDelta = Box<dyn FnMut(&str) + Send>;

/// The tunable limits of a turn, with the defaults of
/// [`ConversationRuntime::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Settings {
    pub(super) system: Option<String>,
    pub(super) max_tokens: u32,
    pub(super) max_iterations: usize,
    pub(super) tool_concurrency: usize,
    pub(super) max_tool_output_chars: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            system: None,
            max_tokens: 8192,
            max_iterations: 25,
            tool_concurrency: 4,
            max_tool_output_chars: 100_000,
        }
    }
}

/// Settings for a [`ConversationRuntime`]: its system prompt, its limits
/// and the receiver of streamed text. Unset values keep the defaults of
/// [`ConversationRuntime::new`].
#[must_use]
pub struct RuntimeBuilder<P, T> {
    provider: P,
    toolbox: T,
    settings: Settings,
    on_delta: Option<OnDelta>,
}

impl<P: LlmProvider, T: Toolbox> RuntimeBuilder<P, T> {
    /// A builder for a runtime on `provider` that offers `toolbox`.
    pub(super) fn new(provider: P, toolbox: T) -> Self {
        Self {
            provider,
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

    /// Sets the `max_tokens` of each request (default 8192).
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.settings.max_tokens = max_tokens;
        self
    }

    /// Sets how many model calls one turn may make (default 25); one more
    /// is [`RuntimeError::MaxIterations`](crate::agent::RuntimeError::MaxIterations).
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

    /// Hands each piece of streamed text to `on_delta` as it arrives.
    pub fn on_delta(mut self, on_delta: impl FnMut(&str) + Send + 'static) -> Self {
        self.on_delta = Some(Box::new(on_delta));
        self
    }

    /// A runtime with these settings and an empty history.
    pub fn build(self) -> ConversationRuntime<P, T> {
        ConversationRuntime::from_parts(self.provider, self.toolbox, self.settings, self.on_delta)
    }
}

// Hand-written: the provider, toolbox and callback need not be `Debug`.
impl<P, T> fmt::Debug for RuntimeBuilder<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeBuilder")
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}
