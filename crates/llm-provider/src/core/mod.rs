//! The core our providers are built on, public so that yours can be too: a
//! [`Transport`] that POSTs a JSON body with your headers and retries before
//! the first content event, and the [`decode`] functions that turn the reply
//! into [`StreamEvent`](crate::StreamEvent)s.
//!
//! An own provider is a type that implements
//! [`LlmProvider`](crate::LlmProvider) by sending its body through a
//! [`Transport`]; see the "Build your own provider" section of the README
//! and `examples/own_provider.rs`.

pub mod decode;
mod transport;

pub use crate::transport::ByteStream;
pub use transport::Transport;
