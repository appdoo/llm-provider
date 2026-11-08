//! [`ProviderOptions`]: the network timeouts every provider is built with.

use std::time::Duration;

/// Network timeouts of a provider: connect 30 s and an idle gap of 90 s
/// unless set.
///
/// ```no_run
/// use std::time::Duration;
/// use llm_provider::ProviderOptions;
///
/// let quick = ProviderOptions::default()
///     .connect_timeout(Duration::from_secs(5))
///     .idle_gap(Duration::from_secs(30));
/// # let _ = quick;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderOptions {
    pub(crate) connect_timeout: Duration,
    pub(crate) idle_gap: Duration,
}

/// Hand-written because the defaults are not zero.
impl Default for ProviderOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            idle_gap: Duration::from_secs(90),
        }
    }
}

impl ProviderOptions {
    /// Sets the limit for establishing the TCP/TLS connection.
    #[must_use]
    pub fn connect_timeout(mut self, limit: Duration) -> Self {
        self.connect_timeout = limit;
        self
    }

    /// Sets the longest silence allowed before the response headers and
    /// between body chunks.
    #[must_use]
    pub fn idle_gap(mut self, gap: Duration) -> Self {
        self.idle_gap = gap;
        self
    }
}
