//! [`TextStream`]: the text of a provider's event stream, delta by delta.

use std::fmt;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use tokio_stream::Stream;

use crate::provider::error::Error;
use crate::provider::{EventStream, StopReason, StreamEvent};

/// The text of a streamed answer, one delta at a time, started by
/// [`Request::stream`](crate::provider::Request::stream).
///
/// Tool calls, token counts and stop reasons are skipped; ask
/// [`Request::events`](crate::provider::Request::events) for those. The one
/// stop reason not skipped is [`StopReason::MaxTokens`], which ends the
/// stream with [`Error::Truncated`]. An error is yielded once and ends the
/// stream.
///
/// `TextStream` implements `Stream`; `next` comes from the caller's own
/// `futures::StreamExt`.
///
/// ```no_run
/// use futures::StreamExt;
/// use llm_provider::Anthropic;
///
/// # async fn run() -> Result<(), llm_provider::Error> {
/// let claude = Anthropic::new("sk-ant-…").llm(Anthropic::CLAUDE_SONNET_5_5);
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

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        while let Some(events) = this.events.as_mut() {
            match ready!(events.as_mut().poll_next(cx)) {
                Some(Ok(StreamEvent::ContentDelta(text))) => return Poll::Ready(Some(Ok(text))),
                Some(Ok(StreamEvent::Stop(StopReason::MaxTokens))) => {
                    this.events = None;
                    return Poll::Ready(Some(Err(Error::Truncated)));
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    // Fused here: an inner stream may have more after an error.
                    this.events = None;
                    return Poll::Ready(Some(Err(error)));
                }
                None => this.events = None,
            }
        }

        Poll::Ready(None)
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
