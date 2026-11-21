//! Retry for transient failures: a [`RetryPolicy`] and the loops that apply
//! it to a one-shot call ([`run`]) or a stream ([`retried`]). The
//! schedule comes from `backon`; which errors retry, `Retry-After` and the
//! rule that a stream retries only before its first content event are ours.

mod policy;
mod run;
mod stream;
mod window;

pub use policy::RetryPolicy;
pub(crate) use run::{run, Sleeper, TokioSleeper};
pub(crate) use stream::retried;

#[cfg(test)]
mod tests;
