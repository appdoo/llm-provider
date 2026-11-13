//! Retry for transient failures: a [`RetryPolicy`] and the [`Retrying`]
//! decorator that applies it to every call of a protocol's API. The
//! schedule comes from `backon`; which errors retry, `Retry-After` and the
//! rule that a stream retries only before its first content event are ours.

mod policy;
mod retrying;
mod run;
mod window;

pub use policy::RetryPolicy;
pub(crate) use retrying::Retrying;
pub(crate) use run::{Sleeper, TokioSleeper};

#[cfg(test)]
mod tests;
