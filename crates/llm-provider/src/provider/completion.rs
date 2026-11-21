//! [`Completion`]: one whole answer, and [`fold`], which builds it from a
//! provider's event stream.

use std::ops::AddAssign;

use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::provider::error::Error;
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent};

/// Token counts of one completion. The cache counters report prompt-cache
/// writes and reads; providers without caching report 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names)] // the APIs name every counter `*_tokens`
#[non_exhaustive]
pub struct Usage {
    /// Input tokens read without the cache.
    pub input_tokens: u32,
    /// Output tokens written.
    pub output_tokens: u32,
    /// Input tokens written to the prompt cache.
    pub cache_creation_input_tokens: u32,
    /// Input tokens read from the prompt cache.
    pub cache_read_input_tokens: u32,
}

impl Usage {
    /// Sets the input tokens read without the cache.
    #[must_use]
    pub fn input_tokens(self, input_tokens: u32) -> Self {
        Self {
            input_tokens,
            ..self
        }
    }

    /// Sets the output tokens written.
    #[must_use]
    pub fn output_tokens(self, output_tokens: u32) -> Self {
        Self {
            output_tokens,
            ..self
        }
    }

    /// Sets the input tokens written to the prompt cache.
    #[must_use]
    pub fn cache_creation_input_tokens(self, cache_creation_input_tokens: u32) -> Self {
        Self {
            cache_creation_input_tokens,
            ..self
        }
    }

    /// Sets the input tokens read from the prompt cache.
    #[must_use]
    pub fn cache_read_input_tokens(self, cache_read_input_tokens: u32) -> Self {
        Self {
            cache_read_input_tokens,
            ..self
        }
    }
}

/// Hand-written because each counter adds saturating, which no derive does.
impl AddAssign for Usage {
    fn add_assign(&mut self, more: Self) {
        self.input_tokens = self.input_tokens.saturating_add(more.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(more.output_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .saturating_add(more.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .saturating_add(more.cache_read_input_tokens);
    }
}

/// A tool call the model made.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ToolUseCall {
    /// The call's id, which its result refers to.
    pub id: String,
    /// The name of the tool called.
    pub name: String,
    /// The tool's input as the model wrote it.
    pub input: serde_json::Value,
}

impl ToolUseCall {
    /// A call with id `id` of the tool `name` on `input`.
    ///
    /// ```
    /// use llm_provider::provider::ToolUseCall;
    /// use serde_json::json;
    ///
    /// let call = ToolUseCall::new("t1", "clock", json!({}));
    /// assert_eq!(call.name, "clock");
    /// ```
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
        }
    }
}

/// One whole answer: its text, tool calls, token counts and stop reason.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Completion {
    /// The answer text.
    pub text: String,
    /// The tool calls, in order.
    pub tool_uses: Vec<ToolUseCall>,
    /// The summed token counts.
    pub usage: Usage,
    /// Why the answer ended.
    pub stop: StopReason,
}

/// The completion `events` add up to, handing each text delta to
/// `on_delta`, raced against `cancel`. Without a [`StreamEvent::Stop`] the
/// stop reason is [`StopReason::ToolUse`] when a tool was called, else
/// [`StopReason::EndTurn`].
/// A token already cancelled returns before the first poll, so no request
/// is sent. Each item polls the stream first, so a
/// [`StreamEvent::MessageStop`] ready together with the cancel wins.
/// Returning drops `events`, which closes the request.
///
/// # Errors
/// [`Error::Interrupted`] with the text received so far when `cancel`
/// fires; otherwise the first error of the stream, or
/// [`Error::UnexpectedEnd`] when it ends before [`StreamEvent::MessageStop`].
pub(crate) async fn fold_cancellable(
    events: EventStream,
    on_delta: impl FnMut(&str) + Send,
    cancel: &CancellationToken,
) -> Result<Completion, Error> {
    // The stream opens its request on the first poll: a cancelled token
    // returns before it, so nothing is sent.
    if cancel.is_cancelled() {
        return Err(Error::Interrupted {
            partial_text: String::new(),
        });
    }

    let mut events = events;
    let mut on_delta = on_delta;
    let mut partial = Partial::default();
    loop {
        let event = tokio::select! {
            biased;
            event = events.next() => event,
            () = cancel.cancelled() => {
                return Err(Error::Interrupted { partial_text: partial.text });
            }
        };
        let Some(event) = event else {
            return Err(Error::UnexpectedEnd);
        };
        if partial.add(event?, &mut on_delta) {
            return Ok(partial.finish());
        }
    }
}

/// The parts of a completion received so far.
#[derive(Default)]
struct Partial {
    text: String,
    tool_uses: Vec<ToolUseCall>,
    usage: Usage,
    stop: Option<StopReason>,
}

impl Partial {
    /// Adds `event`, handing a text delta to `on_delta`; `true` on
    /// [`StreamEvent::MessageStop`].
    fn add(&mut self, event: StreamEvent, on_delta: &mut impl FnMut(&str)) -> bool {
        match event {
            StreamEvent::ContentDelta(delta) => {
                on_delta(&delta);
                self.text.push_str(&delta);
            }
            StreamEvent::ToolUse(call) => self.tool_uses.push(call),
            StreamEvent::Usage(more) => self.usage += more,
            StreamEvent::Stop(reason) => self.stop = Some(reason),
            StreamEvent::MessageStop => return true,
        }
        false
    }

    /// The completion, inferring the stop reason when none was reported.
    fn finish(self) -> Completion {
        let stop = self.stop.unwrap_or_else(|| inferred_stop(&self.tool_uses));
        Completion {
            text: self.text,
            tool_uses: self.tool_uses,
            usage: self.usage,
            stop,
        }
    }
}

/// The stop reason of an answer that reported none: `ToolUse` when it
/// called a tool, else `EndTurn`.
fn inferred_stop(tool_uses: &[ToolUseCall]) -> StopReason {
    if tool_uses.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolUse
    }
}

#[cfg(test)]
mod tests;
