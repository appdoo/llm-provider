//! The OpenAI `/embeddings` wire: one attempt, its body and its answer.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::provider::embedder::EmbedProtocol;
use crate::provider::error::Error;
use crate::provider::openai::api::OpenAiApi;

/// The path of the embeddings endpoint below the API root.
const EMBEDDINGS_PATH: &str = "/embeddings";

/// The request body: the model and the texts.
#[derive(Debug, Serialize)]
struct EmbeddingsBody<'a> {
    model: &'a str,
    input: &'a [String],
}

/// The answer: one vector per input, each tagged with its input's index.
#[derive(Debug, Deserialize)]
struct EmbeddingsAnswer {
    data: Vec<Embedding>,
}

/// One vector and the index of the input it belongs to.
#[derive(Debug, Deserialize)]
struct Embedding {
    index: usize,
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbedProtocol for OpenAiApi {
    /// Retries every retryable error under the channel's policy.
    async fn embed(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        self.channel()
            .call(Error::is_retryable, || self.embed_once(model, texts))
            .await
    }
}

impl OpenAiApi {
    /// One attempt: posts the texts to [`EMBEDDINGS_PATH`] and returns the
    /// vectors sorted by their input's index.
    async fn embed_once(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, Error> {
        let body = EmbeddingsBody {
            model,
            input: texts,
        };
        let post = self.post(EMBEDDINGS_PATH).await?.json(&body);
        let answer: EmbeddingsAnswer = self.channel().http().json(post).await?;

        let mut data = answer.data;
        data.sort_by_key(|item| item.index);
        Ok(data.into_iter().map(|item| item.embedding).collect())
    }
}
