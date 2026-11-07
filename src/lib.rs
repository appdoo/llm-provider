#![doc = include_str!("../README.md")]
#![deny(unreachable_pub)]
#![forbid(unsafe_code)]

pub mod provider;

#[cfg(feature = "agent")]
pub mod agent;

#[cfg(feature = "fs-tools")]
pub mod fs_tools;

pub use provider::{
    Anthropic, AuthMethod, Completion, CompletionRequest, Error, ErrorKind, LlmProvider, Message,
    OpenAiCompatible, ProviderOptions, Request, RetryPolicy,
};
