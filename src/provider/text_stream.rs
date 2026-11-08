//! [`TextStream`]: the text of a provider's event stream, delta by delta.

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio_stream::Stream;

use crate::provider::error::Error;
use crate::provider::EventStream;

/// The text of a streamed answer, one delta at a time, started by
/// [`Request::stream`](crate::provider::Request::stream).
///
/// Tool calls, token counts and stop reasons are skipped; ask
/// [`Request::events`](crate::provider::Request::events) for those. An error
/// is yielded once and ends the stream.
///
/// `TextStream` implements `Stream`; `next` comes from the caller's own
/// `futures::StreamExt`.
///
/// ```no_run
/// use futures::StreamExt;
/// use llm_provider::Anthropic;
///
/// # async fn run() -> Result<(), llm_provider::Error> {
/// let claude = Anthropic::new("sk-ant-…", "claude-sonnet-5-5");
/// let mut text = claude.request("Tell a short story about a lighthouse.").stream();
/// while let Some(delta) = text.next().await {
///     print!("{}", delta?);
/// }
/// # Ok(()) }
/// ```
#[must_use = "a stream does nothing until it is polled"]
pub struct TextStream {
    /// The provider's events; `None` once the stream has ended.
    events: Option<EventStream>,
}

impl TextStream {
    /// The text of `events`.
    pub(crate) fn new(events: EventStream) -> Self {
        Self {
            events: Some(events),
        }
    }
}

impl Stream for TextStream {
    type Item = Result<String, Error>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

// Hand-written: the boxed `dyn Stream` inside has no `Debug`.
impl fmt::Debug for TextStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextStream")
            .field("ended", &self.events.is_none())
            .finish()
    }
}

#[cfg(test)]
mod tests;
