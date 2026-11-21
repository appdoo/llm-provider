//! [`OpenAi`]: an endpoint speaking the OpenAI Chat Completions and
//! embeddings APIs, which hands out one [`Llm`] or [`Embedder`] per model.

use std::fmt;
use std::sync::Arc;

use crate::config::ConfigError;
use crate::provider::auth::AuthMethod;
use crate::provider::channel::{fixed_origin, parse_origin, Route};
use crate::provider::embedder::Embedder;
use crate::provider::endpoint_builder::Routed;
use crate::provider::llm::Llm;
use crate::provider::openai::api::OpenAiApi;
use crate::provider::openai::builder::OpenAiBuilder;
use crate::provider::openai::completions::CompletionsApi;
use crate::provider::raw::RawLlm;

/// The address of the OpenAI API.
const OPENAI_API: &str = "https://api.openai.com/v1";

/// An endpoint speaking the OpenAI Chat Completions API, retrying transient
/// failures: the OpenAI API, any compatible server, or Bedrock's route from
/// [`Bedrock::openai`](crate::Bedrock::openai). Clones share one HTTP
/// connection pool, and so does every [`Llm`] and [`Embedder`] it hands out.
///
/// Message batches are Anthropic's alone; an `OpenAi` has none:
///
/// ```compile_fail
/// use llm_provider::OpenAi;
///
/// let k = "k";
/// let batches = OpenAi::new(k).batches("x");
/// ```
#[derive(Clone)]
pub struct OpenAi {
    api: Arc<OpenAiApi>,
}

impl OpenAi {
    /// The OpenAI API with `key`.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let openai = OpenAi::new("sk-…");
    /// # let _ = openai;
    /// ```
    #[must_use]
    pub fn new(key: impl Into<AuthMethod>) -> Self {
        OpenAiBuilder::new(fixed_origin(OPENAI_API))
            .credential(key)
            .build()
    }

    /// The server at `url` with the defaults and no credential.
    /// `url` is the whole API root, such as `http://localhost:11434/v1`; a
    /// trailing `/` is ignored.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let ollama = OpenAi::at("http://localhost:11434/v1")?;
    /// # let _ = ollama;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidOrigin`] when `url` does not parse as an
    /// absolute URL, [`ConfigError::UnsupportedScheme`] when its scheme is
    /// neither `http` nor `https`.
    pub fn at(url: impl Into<String>) -> Result<Self, ConfigError> {
        Ok(Self::builder(url)?.build())
    }

    /// A builder for the server at `url` with a credential, timeouts or a
    /// retry policy.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let openai = OpenAi::builder("https://api.openai.com/v1")?
    ///     .credential("sk-…")
    ///     .build();
    /// # let _ = openai;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidOrigin`] when `url` does not parse as an
    /// absolute URL, [`ConfigError::UnsupportedScheme`] when its scheme is
    /// neither `http` nor `https`.
    pub fn builder(url: impl Into<String>) -> Result<OpenAiBuilder, ConfigError> {
        Ok(OpenAiBuilder::new(parse_origin(&url.into())?))
    }

    /// The chat model `model` on this endpoint.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let qwen = OpenAi::at("http://localhost:11434/v1")?.llm("qwen3-coder:30b");
    /// # let _ = qwen;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    #[must_use]
    pub fn llm(&self, model: impl Into<String>) -> Llm {
        Llm::new(self.api.clone(), model)
    }

    /// The raw completion model `model` on this endpoint, asking
    /// `/completions` with a prompt the caller rendered itself.
    ///
    /// `.raw()` also exists on [`Bedrock::openai`](crate::Bedrock::openai),
    /// where every request answers
    /// `Error::Unsupported("Bedrock has no raw completion")` before any I/O.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let qwen = OpenAi::at("http://localhost:8080/v1")?.raw("qwen3-8b");
    /// # let _ = qwen;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    #[must_use]
    pub fn raw(&self, model: impl Into<String>) -> RawLlm {
        let completions = CompletionsApi::new((*self.api).clone());

        RawLlm::new(Arc::new(completions), model)
    }

    /// The embedding model `model` on this endpoint, asking `/embeddings`.
    ///
    /// ```no_run
    /// use llm_provider::OpenAi;
    ///
    /// let bge = OpenAi::at("http://localhost:11434/v1")?.embedder("bge-m3");
    /// # let _ = bge;
    /// # Ok::<(), llm_provider::config::ConfigError>(())
    /// ```
    #[must_use]
    pub fn embedder(&self, model: impl Into<String>) -> Embedder {
        Embedder::new(self.api.clone(), model)
    }
}

impl Routed for OpenAi {
    /// The endpoint on `route`: the origin itself for a server named by its
    /// whole API root, `/openai/v1` on Bedrock.
    fn routed(route: Route) -> Self {
        Self {
            api: Arc::new(OpenAiApi::new(route)),
        }
    }
}

/// Hand-written so that nothing but the API root is printed.
impl fmt::Debug for OpenAi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAi")
            .field("base_url", &self.api.base_url())
            .finish()
    }
}

#[cfg(test)]
mod tests;
