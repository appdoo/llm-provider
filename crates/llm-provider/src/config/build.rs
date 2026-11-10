//! `build()`: the endpoint a [`ModelBuilder`] names, and its model.

use super::{ConfigError, ModelBuilder, Protocol};
use crate::provider::{Anthropic, Bedrock, Embedder, EndpointBuilder, Error, Llm, OpenAi};

/// The Anthropic model a builder without `model` builds.
const DEFAULT_ANTHROPIC_MODEL: &str = Anthropic::CLAUDE_SONNET_5_5;

impl ModelBuilder<Llm> {
    /// The chat model these values name. Builds its own endpoint and sends
    /// nothing.
    ///
    /// # Errors
    /// [`Error::Config`] for a missing credential, endpoint or model.
    pub fn build(self) -> Result<Llm, Error> {
        let model = self.model_id()?;

        match self.protocol {
            Protocol::Anthropic => Ok(self.anthropic_endpoint()?.llm(model)),
            Protocol::OpenAi => Ok(self.openai_endpoint()?.llm(model)),
        }
    }
}

impl ModelBuilder<Embedder> {
    /// The embedding model these values name. Builds its own endpoint and
    /// sends nothing.
    ///
    /// # Errors
    /// [`Error::Unsupported`] for [`Protocol::Anthropic`], which has no
    /// embedding models; [`Error::Config`] for a missing credential,
    /// endpoint or model.
    pub fn build(self) -> Result<Embedder, Error> {
        if self.protocol == Protocol::Anthropic {
            return Err(Error::Unsupported("Anthropic has no embedding models"));
        }
        let model = self.model_id()?;

        Ok(self.openai_endpoint()?.embedder(model))
    }
}

impl<T> ModelBuilder<T> {
    /// The Messages API, direct or on Bedrock; either needs a credential.
    fn anthropic_endpoint(&self) -> Result<Anthropic, ConfigError> {
        if let Some(region) = &self.region {
            return Ok(self.bedrock_endpoint(region)?.anthropic());
        }
        let credential = self
            .credential
            .clone()
            .ok_or(ConfigError::MissingCredential {
                provider: "anthropic",
            })?;

        let mut builder = self.configured(Anthropic::builder(credential));
        if let Some(url) = &self.endpoint {
            builder = builder.base_url(url);
        }

        Ok(builder.build())
    }

    /// The Chat Completions API on Bedrock, which needs a credential, or on
    /// a server, which needs an endpoint and sends a credential when set.
    fn openai_endpoint(&self) -> Result<OpenAi, ConfigError> {
        if let Some(region) = &self.region {
            return Ok(self.bedrock_endpoint(region)?.openai());
        }
        let url = self
            .endpoint
            .as_deref()
            .ok_or(ConfigError::MissingEndpoint { provider: "openai" })?;

        let mut builder = self.configured(OpenAi::builder(url));
        if let Some(credential) = self.credential.clone() {
            builder = builder.credential(credential);
        }

        Ok(builder.build())
    }

    /// `builder` with these values' timeouts and retry policy.
    fn configured<E>(&self, builder: EndpointBuilder<E>) -> EndpointBuilder<E> {
        builder.timeouts(self.options()).retry(self.retry_policy())
    }

    /// The model id: the one set, or for the Anthropic API the default.
    fn model_id(&self) -> Result<String, ConfigError> {
        if let Some(model) = &self.model {
            return Ok(model.clone());
        }

        match (self.protocol, &self.region) {
            (Protocol::Anthropic, None) => Ok(DEFAULT_ANTHROPIC_MODEL.to_string()),
            (_, Some(_)) => Err(ConfigError::MissingModel {
                provider: "bedrock",
            }),
            (Protocol::OpenAi, None) => Err(ConfigError::MissingModel { provider: "openai" }),
        }
    }

    /// The Bedrock endpoint in `region`, which needs a credential.
    fn bedrock_endpoint(&self, region: &str) -> Result<Bedrock, ConfigError> {
        let credential = self
            .credential
            .clone()
            .ok_or(ConfigError::MissingCredential {
                provider: "bedrock",
            })?;

        let mut builder = self.configured(Bedrock::builder(credential, region));
        if let Some(origin) = &self.endpoint {
            builder = builder.base_url(origin);
        }

        Ok(builder.build())
    }
}
