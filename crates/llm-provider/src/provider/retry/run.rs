//! Retrying one call: the [`Sleeper`] between attempts, the wait before the
//! next one and the error a call gives up with.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use backon::Retryable;

use super::RetryPolicy;
use crate::provider::error::Error;

/// Waits between attempts. Tests substitute a recorder.
#[async_trait]
pub(crate) trait Sleeper: Send + Sync + fmt::Debug {
    /// Waits for `duration`.
    async fn sleep(&self, duration: Duration);
}

/// The [`Sleeper`] on `tokio::time::sleep`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// A [`Sleeper`] in the shape `backon` takes: a function from a wait to a
/// future, so tests can swap the clock.
fn sleep_fn(
    sleeper: Arc<dyn Sleeper>,
) -> impl Fn(Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static {
    move |wait| {
        let sleeper = Arc::clone(&sleeper);
        Box::pin(async move { sleeper.sleep(wait).await })
    }
}

/// The wait before the next attempt after `error`: the server's
/// `Retry-After` when the answer carries one, else `scheduled` from the
/// backoff. `None` stops: the schedule ran out, or `Retry-After` exceeds the
/// policy's cap. This is `backon`'s `adjust` hook.
pub(crate) fn next_wait(
    policy: &RetryPolicy,
    error: &Error,
    scheduled: Option<Duration>,
) -> Option<Duration> {
    let scheduled = scheduled?;
    let asked = match error {
        Error::Api(api) => api.retry_after(),
        _ => None,
    };

    match asked {
        Some(wait) => (wait <= policy.longest_wait()).then_some(wait),
        None => Some(scheduled),
    }
}

/// Runs `attempt` with `backon` until it succeeds, `retry_if` says the error
/// is final, or [`next_wait`] says stop. A final error comes back unchanged;
/// a retryable one that ran out is [`Error::RetriesExhausted`] with the
/// number of attempts made.
pub(crate) async fn run<T, F, Fut>(
    policy: &RetryPolicy,
    sleeper: Arc<dyn Sleeper>,
    retry_if: fn(&Error) -> bool,
    attempt: F,
) -> Result<T, Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Error>>,
{
    let mut waits: u32 = 0;
    let result = attempt
        .retry(policy.backoff())
        .sleep(sleep_fn(sleeper))
        .when(retry_if)
        .adjust(|error, scheduled| next_wait(policy, error, scheduled))
        .notify(|_, _| waits += 1)
        .await;

    result.map_err(|error| give_up(error, waits + 1, retry_if))
}

/// The error a call ends with: a retryable one becomes
/// [`Error::RetriesExhausted`] after `attempts`, a final one stays as it is.
pub(crate) fn give_up(error: Error, attempts: u32, retry_if: fn(&Error) -> bool) -> Error {
    if !retry_if(&error) {
        return error;
    }

    Error::RetriesExhausted {
        attempts,
        last: Box::new(error),
    }
}
