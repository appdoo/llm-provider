//! [`Completion`]: one whole answer, and [`fold`], which builds it from a
//! provider's event stream.

use std::ops::AddAssign;

use tokio_stream::StreamExt;

use crate::provider::error::Error;
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent};

/// Token counts of one completion. The cache counters report prompt-cache
/// writes and reads; providers without caching report 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_field_names)] // the APIs name every counter `*_tokens`
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
        let _ = input_tokens;
        todo!()
    }

    /// Sets the output tokens written.
    #[must_use]
    pub fn output_tokens(self, output_tokens: u32) -> Self {
        let _ = output_tokens;
        todo!()
    }

    /// Sets the input tokens written to the prompt cache.
    #[must_use]
    pub fn cache_creation_input_tokens(self, cache_creation_input_tokens: u32) -> Self {
        let _ = cache_creation_input_tokens;
        todo!()
    }

    /// Sets the input tokens read from the prompt cache.
    #[must_use]
    pub fn cache_read_input_tokens(self, cache_read_input_tokens: u32) -> Self {
        let _ = cache_read_input_tokens;
        todo!()
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
    /// ```no_run
    /// use llm_provider::provider::ToolUseCall;
    /// use serde_json::json;
    ///
    /// let call = ToolUseCall::new("t1", "clock", json!({}));
    /// # let _ = call;
    /// ```
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, input: serde_json::Value) -> Self {
        drop((id, name, input));
        todo!()
    }
}

/// One whole answer: its text, tool calls, token counts and stop reason.
#[derive(Debug, Clone, PartialEq)]
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
/// `on_delta`. Without a [`StreamEvent::Stop`] the stop reason is
/// [`StopReason::ToolUse`] when a tool was called, else
/// [`StopReason::EndTurn`].
///
/// # Errors
/// The first error of the stream; [`Error::UnexpectedEnd`] when it ends
/// before [`StreamEvent::MessageStop`].
pub(crate) async fn fold(
    mut events: EventStream,
    mut on_delta: impl FnMut(&str) + Send,
) -> Result<Completion, Error> {
    let mut text = String::new();
    let mut tool_uses = Vec::new();
    let mut usage = Usage::default();
    let mut stop = None;

    while let Some(event) = events.next().await {
        match event? {
            StreamEvent::ContentDelta(delta) => {
                on_delta(&delta);
                text.push_str(&delta);
            }
            StreamEvent::ToolUse(call) => tool_uses.push(call),
            StreamEvent::Usage(more) => usage += more,
            StreamEvent::Stop(reason) => stop = Some(reason),
            StreamEvent::MessageStop => {
                let stop = stop.unwrap_or_else(|| inferred_stop(&tool_uses));
                return Ok(Completion {
                    text,
                    tool_uses,
                    usage,
                    stop,
                });
            }
        }
    }

    Err(Error::UnexpectedEnd)
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
