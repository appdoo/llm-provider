//! Retry for transient provider failures: a [`RetryPolicy`] and the
//! [`RetryingProvider`] decorator that applies it to every call of a
//! provider. The schedule comes from `backon`; which errors retry,
//! `Retry-After` and the rule that a stream retries only before its first
//! content event are ours.

mod policy;
mod provider;
mod run;
mod window;

pub use policy::RetryPolicy;
pub(crate) use provider::RetryingProvider;
#[cfg(test)]
pub(crate) use run::Sleeper;

#[cfg(test)]
mod tests;
