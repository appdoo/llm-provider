//! The `/completions` protocol: a raw prompt as the body, the reply decoded
//! from `text` chunks. [`CompletionsBody`] is the body of a
//! [`RawRequest`](crate::RawRequest), [`CompletionsApi`] its one attempt over
//! the wire.

mod api;
mod body;
pub(crate) mod stream;

pub(super) use api::CompletionsApi;
pub(crate) use body::CompletionsBody;

#[cfg(test)]
mod cancel_tests;
#[cfg(test)]
mod tests;
