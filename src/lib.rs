#![doc = include_str!("../README.md")]
#![warn(clippy::pedantic)]
#![deny(unreachable_pub)]
#![forbid(unsafe_code)]

pub mod provider;
mod transport;

#[cfg(feature = "agent")]
pub mod agent;

#[cfg(feature = "fs-tools")]
pub mod fs_tools;

pub use provider::{
    Anthropic, AuthMethod, Completion, CompletionRequest, ConfigError, Error, ErrorKind,
    LlmProvider, Message, OpenAiCompatible, ProviderOptions, Request, RetryPolicy,
};
