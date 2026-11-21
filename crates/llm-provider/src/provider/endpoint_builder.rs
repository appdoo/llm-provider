//! [`EndpointBuilder`]: settings for an endpoint, collected before it is
//! built. The setters every endpoint shares live here; the ones only one
//! endpoint has live next to it ([`AnthropicBuilder`](crate::provider::AnthropicBuilder),
//! [`OpenAiBuilder`](crate::provider::OpenAiBuilder),
//! [`BedrockBuilder`](crate::provider::BedrockBuilder)).
//!
//! ```no_run
//! use llm_provider::{Anthropic, RetryPolicy};
//!
//! let claude = Anthropic::builder("sk-ant-…")
//!     .retry(RetryPolicy::none())
//!     .build();
//! # let _ = claude;
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use crate::provider::channel::{Channel, ChannelParts, Route};
use crate::provider::options::ProviderOptions;
use crate::provider::retry::RetryPolicy;
#[cfg(test)]
use crate::provider::retry::Sleeper;

/// An endpoint built from a [`Channel`]: what [`EndpointBuilder::build`]
/// makes.
pub(crate) trait FromChannel {
    /// The endpoint on `channel`.
    fn from_channel(channel: Channel) -> Self;
}

/// An endpoint made from a [`Route`]: the root route of a channel, or a
/// route below it such as Bedrock's.
pub(crate) trait Routed {
    /// The endpoint on `route`.
    fn routed(route: Route) -> Self;
}

impl<E: Routed> FromChannel for E {
    fn from_channel(channel: Channel) -> Self {
        E::routed(Route::root(Arc::new(channel)))
    }
}

/// Settings for an endpoint `E`: [`Anthropic`](crate::Anthropic),
/// [`OpenAi`](crate::OpenAi) or [`Bedrock`](crate::Bedrock). Unset values
/// take the defaults: [`ProviderOptions::default`] (connect 30 s, idle
/// 90 s) and [`RetryPolicy::default`] (4 attempts).
#[derive(Clone, Debug)]
#[must_use]
pub struct EndpointBuilder<E> {
    parts: ChannelParts,
    builds: PhantomData<fn() -> E>,
}

impl<E> EndpointBuilder<E> {
    /// A builder for `E` from `parts`.
    pub(crate) fn from_parts(parts: ChannelParts) -> Self {
        Self {
            parts,
            builds: PhantomData,
        }
    }

    /// The parts an endpoint's own setters fill in.
    pub(crate) fn parts_mut(&mut self) -> &mut ChannelParts {
        &mut self.parts
    }

    /// Sets the connect timeout and the longest silence on a response.
    pub fn timeouts(mut self, options: ProviderOptions) -> Self {
        self.parts.options = options;
        self
    }

    /// Sends through `client`, one the caller configured with a proxy, root
    /// certificates or a shared pool, instead of a client built from
    /// [`timeouts`](Self::timeouts). Its timeouts and redirect policy are the
    /// caller's; the client this crate builds follows no redirect, so the
    /// credential never reaches a second origin.
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.parts.client = Some(client);
        self
    }

    /// Sets the retry policy; [`RetryPolicy::none`] turns retry off.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.parts.retry = policy;
        self
    }

    /// Waits between attempts through `sleeper` instead of
    /// `tokio::time::sleep`.
    #[cfg(test)]
    pub(crate) fn sleeper(mut self, sleeper: Arc<dyn Sleeper>) -> Self {
        self.parts.sleeper = sleeper;
        self
    }
}

#[allow(private_bounds)] // sealed: only this crate's endpoints are built from a channel
impl<E: FromChannel> EndpointBuilder<E> {
    /// The endpoint. Its HTTP connection pool is built on first use, so
    /// building never fails and never touches the network.
    #[must_use]
    pub fn build(self) -> E {
        E::from_channel(Channel::new(self.parts))
    }
}

#[cfg(test)]
mod tests;
