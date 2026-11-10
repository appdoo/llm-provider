//! [`Embedder`]: one embedding model behind one protocol client, the type a
//! program asks for vectors; [`Embed`] is what it does.
//!
//! ```no_run
//! use llm_provider::{Embed, OpenAi};
//!
//! # async fn run() -> Result<(), llm_provider::Error> {
//! let ollama = OpenAi::at("http://localhost:11434/v1");
//! let bge = ollama.embedder("bge-m3");
//! let vectors = bge.embed(&["a cat".to_string(), "a dog".to_string()]).await?;
//! # let _ = vectors; Ok(()) }
//! ```

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::ModelBuilder;
use crate::provider::error::Error;

/// Turns texts into vectors, in input order.
#[async_trait]
pub trait Embed: Send + Sync {
    /// The vectors of `texts`, in input order; `embed(&[])` is `Ok(vec![])`
    /// and sends no request.
    ///
    /// # Errors
    /// Any [`Error`] of the request.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, Error>;
}

/// One way of asking for embeddings, without a model of its own: the caller
/// names the model for each request.
#[async_trait]
pub(crate) trait EmbedProtocol: Send + Sync {
    /// The vectors of `texts` from `model`, in input order.
    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error>;
}

/// An embedding model on a protocol client. Clones, and every model from
/// one client, share that client's connection pool and retry policy.
///
/// ```no_run
/// use llm_provider::{Embed, OpenAi};
///
/// # async fn run() -> Result<(), llm_provider::Error> {
/// let bge = OpenAi::at("http://localhost:11434/v1").embedder("bge-m3");
/// let vectors = bge.embed(&["hello".to_string()]).await?;
/// # let _ = vectors; Ok(()) }
/// ```
///
/// Only an endpoint whose protocol serves embeddings hands one out; an
/// [`Anthropic`](crate::Anthropic) has none:
///
/// ```compile_fail
/// use llm_provider::Anthropic;
///
/// let k = "k";
/// let embedder = Anthropic::new(k).embedder("x");
/// ```
#[derive(Clone)]
pub struct Embedder {
    protocol: Arc<dyn EmbedProtocol>,
    model: String,
}

impl Embedder {
    /// The model `model` on `protocol`.
    pub(crate) fn new(protocol: Arc<dyn EmbedProtocol>, model: impl Into<String>) -> Self {
        Self {
            protocol,
            model: model.into(),
        }
    }

    /// A builder for the embedding model `model`, reached by values known
    /// at run time: protocol, endpoint, region and credential. Only
    /// [`Protocol::OpenAi`](crate::Protocol::OpenAi) serves embeddings.
    ///
    /// ```no_run
    /// use llm_provider::{Embedder, Protocol};
    ///
    /// # fn run() -> Result<(), llm_provider::Error> {
    /// let bge = Embedder::builder("bge-m3")
    ///     .protocol(Protocol::OpenAi)
    ///     .endpoint("http://localhost:11434/v1")
    ///     .build()?;
    /// # let _ = bge; Ok(()) }
    /// ```
    pub fn builder(model: impl Into<String>) -> ModelBuilder<Embedder> {
        ModelBuilder::new(model.into())
    }

    /// An embedding model over a caller's own embedder; `model` names it,
    /// since [`Embed`] cannot. It is not wrapped in retry.
    ///
    /// ```no_run
    /// use llm_provider::{Embed, Embedder};
    ///
    /// fn on_device(embed: impl Embed + 'static) -> Embedder {
    ///     Embedder::from_embed(embed, "bge-m3")
    /// }
    /// ```
    #[must_use]
    pub fn from_embed(embed: impl Embed + 'static, model: impl Into<String>) -> Self {
        Self::new(Arc::new(OwnEmbed(embed)), model)
    }

    /// The model id every request names.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

#[async_trait]
impl Embed for Embedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        self.protocol.embed(&self.model, texts).await
    }
}

/// Hand-written because the protocol is a trait object.
impl fmt::Debug for Embedder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Embedder")
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

/// A caller's own embedder as a protocol: it has one model, so the model
/// a request names is not passed on.
struct OwnEmbed<E>(E);

#[async_trait]
impl<E: Embed> EmbedProtocol for OwnEmbed<E> {
    async fn embed(&self, _model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        self.0.embed(texts).await
    }
}

#[cfg(test)]
mod tests;
