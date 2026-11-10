//! [`Bedrock`]: Amazon Bedrock, one channel that speaks two protocols, and
//! its model ids.
//!
//! ```no_run
//! use llm_provider::Bedrock;
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let bedrock = Bedrock::new("ABSK…", "eu-central-1");
//! let haiku = bedrock.anthropic().llm(Bedrock::EU_CLAUDE_HAIKU_5_5);
//! let kimi = bedrock.openai().llm(Bedrock::KIMI_K3);
//! let answer = haiku.ask("Name three primes.").await?;
//! # let _ = (answer, kimi); Ok(()) }
//! ```

use std::sync::Arc;

use crate::provider::anthropic::Anthropic;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::Channel;
use crate::provider::openai::OpenAi;

mod builder;

pub use builder::BedrockBuilder;

/// The route Bedrock serves the Anthropic Messages API under.
const ANTHROPIC_ROUTE: &str = "/anthropic";
/// The route Bedrock serves the OpenAI Chat Completions API under.
const OPENAI_ROUTE: &str = "/openai/v1";

/// Amazon Bedrock in one region with one credential, handing out an
/// endpoint per protocol: [`anthropic`](Self::anthropic) for Claude,
/// [`openai`](Self::openai) for models such as Kimi K3. Both share one HTTP
/// connection pool, retry policy and credential.
#[derive(Clone, Debug)]
pub struct Bedrock {
    channel: Arc<Channel>,
}

impl Bedrock {
    /// Claude Haiku 5.5, the EU cross-region inference profile. A
    /// `Bedrock::*` id is valid on a Bedrock endpoint, an
    /// `Anthropic::CLAUDE_*` id on the Anthropic API; the other way round
    /// the server answers 400.
    pub const EU_CLAUDE_HAIKU_5_5: &'static str = "eu.anthropic.claude-haiku-5-5";
    /// Claude Sonnet 5.5, the EU cross-region inference profile. A
    /// `Bedrock::*` id is valid on a Bedrock endpoint, an
    /// `Anthropic::CLAUDE_*` id on the Anthropic API; the other way round
    /// the server answers 400.
    pub const EU_CLAUDE_SONNET_5_5: &'static str = "eu.anthropic.claude-sonnet-5-5";
    /// Claude Opus 5.5, the EU cross-region inference profile. A
    /// `Bedrock::*` id is valid on a Bedrock endpoint, an
    /// `Anthropic::CLAUDE_*` id on the Anthropic API; the other way round
    /// the server answers 400.
    pub const EU_CLAUDE_OPUS_5_5: &'static str = "eu.anthropic.claude-opus-5-5";
    /// Kimi K3, the global cross-region inference profile, on
    /// [`openai`](Self::openai). A `Bedrock::*` id is valid on a Bedrock
    /// endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API; the other
    /// way round the server answers 400.
    pub const KIMI_K3: &'static str = "global.moonshotai.kimi-k3";

    /// Bedrock in `region` with `key` and the defaults. A plain string is a
    /// Bedrock API key. A region not of the form `eu-central-1` fails the
    /// first request with [`Error::InvalidRegion`](crate::Error::InvalidRegion),
    /// before any I/O.
    ///
    /// ```no_run
    /// use llm_provider::Bedrock;
    ///
    /// let bedrock = Bedrock::new("ABSK…", "eu-central-1");
    /// # let _ = bedrock;
    /// ```
    #[must_use]
    pub fn new(key: impl Into<AuthMethod>, region: impl Into<String>) -> Self {
        Self::builder(key, region).build()
    }

    /// A builder for Bedrock in `region` with another origin, timeouts or
    /// retry policy.
    ///
    /// ```no_run
    /// use llm_provider::{Bedrock, RetryPolicy};
    ///
    /// let bedrock = Bedrock::builder("ABSK…", "eu-central-1")
    ///     .retry(RetryPolicy::none())
    ///     .build();
    /// # let _ = bedrock;
    /// ```
    pub fn builder(key: impl Into<AuthMethod>, region: impl Into<String>) -> BedrockBuilder {
        BedrockBuilder::new(key.into(), region.into())
    }

    /// The Anthropic Messages API on Bedrock, route `/anthropic`: the key
    /// goes as `x-api-key`, a bearer token as `Authorization` without the
    /// OAuth beta header. Message batches are not available here.
    #[must_use]
    pub fn anthropic(&self) -> Anthropic {
        Anthropic::on(&self.channel, ANTHROPIC_ROUTE)
    }

    /// The OpenAI Chat Completions API on Bedrock, route `/openai/v1`; the
    /// credential goes as `Authorization: Bearer`.
    #[must_use]
    pub fn openai(&self) -> OpenAi {
        OpenAi::on(&self.channel, OPENAI_ROUTE)
    }

    /// The endpoint on `channel`; only [`BedrockBuilder::build`] calls it.
    fn on(channel: Channel) -> Self {
        Self {
            channel: Arc::new(channel),
        }
    }
}

#[cfg(test)]
mod tests;
