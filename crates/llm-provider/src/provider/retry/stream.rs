//! A stream retried while only `Usage` has arrived.

use std::sync::Arc;

use tokio_stream::StreamExt;

use super::run::Sleeper;
use super::window::{Attempt, Window};
use super::RetryPolicy;
use crate::provider::EventStream;

/// The stream `start` opens, retried under `policy` while only `Usage` has
/// arrived; after the first content event every item is forwarded, never
/// retried. `sleeper` waits between attempts.
pub(crate) fn retried<F>(policy: RetryPolicy, sleeper: Arc<dyn Sleeper>, start: F) -> EventStream
where
    F: Fn() -> EventStream + Send + Sync + 'static,
{
    Box::pin(async_stream::stream! {
        let mut backoff = policy.backoff();
        let mut number = 1;
        let mut events = loop {
            let attempt = Attempt { policy: &policy, number };
            match attempt.open(&start, &mut backoff).await {
                Window::Open(events) => break events,
                Window::Fail(items) => break Box::pin(tokio_stream::iter(items)) as EventStream,
                Window::Retry(wait) => sleeper.sleep(wait).await,
            }
            number += 1;
        };

        while let Some(item) = events.next().await {
            yield item;
        }
    })
}
