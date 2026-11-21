//! [`Sampling`]: how a request samples its answer.

use serde::{Deserialize, Serialize};

use crate::provider::error::RequestError;

/// How a request samples its answer. An unset value is left to the
/// server's default.
///
/// ```no_run
/// use llm_provider::Sampling;
///
/// let sampling = Sampling::new().temperature(0.2).top_p(0.9).seed(7).stop(["</s>"]);
/// # let _ = sampling;
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Sampling {
    /// The sampling temperature; it must be finite.
    pub temperature: Option<f64>,
    /// The nucleus-sampling probability mass; it must be finite.
    pub top_p: Option<f64>,
    /// The seed of the sampler.
    pub seed: Option<u64>,
    /// The sequences that end the answer.
    pub stop: Option<Vec<String>>,
}

impl Sampling {
    /// Sampling with every value left to the server.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the sampling temperature. A non-finite value is
    /// [`RequestError::NonFinite`] when the request is sent, before any I/O.
    #[must_use]
    pub fn temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Sets the nucleus-sampling probability mass. A non-finite value is
    /// [`RequestError::NonFinite`] when the request is sent, before any I/O.
    #[must_use]
    pub fn top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }

    /// Sets the seed of the sampler. The accepted range is each server's:
    /// llama-server reads a `u32` (larger values wrap, -1 means random),
    /// vLLM an `i64`, Ollama a Go `int`.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Sets the sequences that end the answer; a second call replaces the
    /// list. A finish by a stop sequence reports
    /// [`StopReason::EndTurn`](crate::provider::StopReason::EndTurn): neither
    /// raw wire says which sequence fired.
    #[must_use]
    pub fn stop(mut self, stop: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.stop = Some(stop.into_iter().map(Into::into).collect());
        self
    }

    /// Whether no value is set; such sampling is left out of a serialised
    /// request.
    pub(crate) fn is_unset(&self) -> bool {
        *self == Self::default()
    }

    /// Checks the values before any I/O: a non-finite `temperature` is
    /// [`RequestError::NonFinite`]`("temperature")`, else a non-finite
    /// `top_p` is [`RequestError::NonFinite`]`("top_p")`.
    pub(crate) fn check(&self) -> Result<(), RequestError> {
        let finite = |value: Option<f64>| value.is_none_or(f64::is_finite);

        if !finite(self.temperature) {
            return Err(RequestError::NonFinite("temperature"));
        }
        if !finite(self.top_p) {
            return Err(RequestError::NonFinite("top_p"));
        }
        Ok(())
    }
}
