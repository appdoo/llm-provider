//! The retry window of a stream: an attempt is read up to its first content
//! event, and only an error before it is retried.

use std::time::Duration;

use backon::ExponentialBackoff;
use tokio_stream::StreamExt;

use super::run::next_wait;
use super::RetryPolicy;
use crate::provider::error::Error;
use crate::provider::{EventStream, StreamEvent};

/// How one streaming attempt opened.
pub(super) enum Window {
    /// The first content event arrived: the buffered usage, then the rest
    /// of the stream, never retried.
    Open(EventStream),
    /// A retryable error inside the window: wait, then try again.
    Retry(Duration),
    /// The attempt ended for good: yield these items and stop.
    Fail(Vec<Result<StreamEvent, Error>>),
}

/// One streaming attempt: its policy and the attempts made so far, for
/// [`Error::RetriesExhausted`].
pub(super) struct Attempt<'a> {
    pub(super) policy: &'a RetryPolicy,
    pub(super) number: u32,
}

impl Attempt<'_> {
    /// Opens one attempt with `start` and reads it up to its first content
    /// event (only `Usage` may precede it), then decides: open, retry (the
    /// wait from [`next_wait`] over `backoff`) or fail.
    pub(super) async fn open<F: Fn() -> EventStream>(
        &self,
        start: &F,
        backoff: &mut ExponentialBackoff,
    ) -> Window {
        let mut events = start();
        let mut buffered = Vec::new();

        loop {
            match events.next().await {
                Some(Ok(usage @ StreamEvent::Usage(_))) => buffered.push(usage),
                Some(Ok(event)) => {
                    buffered.push(event);
                    return Window::Open(forward(buffered, events));
                }
                Some(Err(error)) => return self.after_error(error, buffered, backoff),
                None => return Window::Fail(buffered.into_iter().map(Ok).collect()),
            }
        }
    }

    /// The decision after an error inside the window: a final error is
    /// yielded after the buffered usage; a retryable one waits, or ends in
    /// [`Error::RetriesExhausted`] with the buffered usage dropped.
    fn after_error(
        &self,
        error: Error,
        buffered: Vec<StreamEvent>,
        backoff: &mut ExponentialBackoff,
    ) -> Window {
        if !error.is_retryable() {
            let items = buffered.into_iter().map(Ok).chain([Err(error)]);
            return Window::Fail(items.collect());
        }

        match next_wait(self.policy, &error, backoff.next()) {
            Some(wait) => Window::Retry(wait),
            None => Window::Fail(vec![Err(Error::RetriesExhausted {
                attempts: self.number,
                last: Box::new(error),
            })]),
        }
    }
}

/// The opened stream: the buffered events, then the rest.
fn forward(buffered: Vec<StreamEvent>, rest: EventStream) -> EventStream {
    Box::pin(tokio_stream::iter(buffered.into_iter().map(Ok)).chain(rest))
}
