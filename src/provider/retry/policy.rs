//! [`RetryPolicy`]: how often to try and how long to wait.

use std::time::Duration;

use backon::{BackoffBuilder, ExponentialBackoff, ExponentialBuilder};

/// How often to try a request and how long to wait between attempts.
/// The default is 4 attempts, a base wait of 1 s and a cap of 60 s.
///
/// ```no_run
/// use std::time::Duration;
/// use llm_provider::RetryPolicy;
///
/// let patient = RetryPolicy::default().max_attempts(6).cap(Duration::from_secs(120));
/// let once = RetryPolicy::none();
/// # let _ = (patient, once);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u32,
    base: Duration,
    cap: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// One attempt, no retry.
    #[must_use]
    pub fn none() -> Self {
        Self::default().max_attempts(1)
    }

    /// Sets the total number of attempts, the first included; 0 counts as 1.
    #[must_use]
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n.max(1);
        self
    }

    /// Sets the wait before the second attempt, before jitter; each later
    /// wait doubles it.
    #[must_use]
    pub fn base(mut self, wait: Duration) -> Self {
        self.base = wait;
        self
    }

    /// Sets the longest wait; a larger `Retry-After` gives up instead.
    #[must_use]
    pub fn cap(mut self, wait: Duration) -> Self {
        self.cap = wait;
        self
    }

    /// The longest wait the policy accepts.
    pub(crate) fn longest_wait(&self) -> Duration {
        self.cap
    }

    /// The wait schedule between attempts, `max_attempts - 1` waits: the
    /// step doubles from `base` up to `cap`, and each wait lies between half
    /// the step and the step (equal jitter). `backon` adds up to one step of
    /// jitter, so it runs on half steps.
    pub(crate) fn backoff(&self) -> ExponentialBackoff {
        let waits = usize::try_from(self.max_attempts - 1).unwrap_or(usize::MAX);
        ExponentialBuilder::default()
            .with_min_delay(self.base / 2)
            .with_max_delay(self.cap / 2)
            .with_max_times(waits)
            .with_jitter()
            .build()
    }
}
