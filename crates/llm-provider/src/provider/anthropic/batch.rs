//! [`Batches`]: message batches on the Messages API for one model, and the
//! one-attempt calls that [`Batches`] sends through its endpoint's retry
//! policy. The wire format is in `wire`.
//!
//! ```no_run
//! use llm_provider::provider::{Anthropic, BatchProvider};
//!
//! # use llm_provider::provider::BatchRequest;
//! # async fn run(requests: Vec<BatchRequest>) -> Result<(), llm_provider::Error> {
//! let batches = Anthropic::new("sk-ant-…").batches(Anthropic::CLAUDE_HAIKU_5_5);
//! let id = batches.submit(requests).await?;
//! # let _ = id; Ok(()) }
//! ```

mod wire;

use async_trait::async_trait;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::provider::anthropic::api::AnthropicApi;
use crate::provider::batch::{
    BatchId, BatchProvider, BatchRequest, BatchResult, BatchState, BatchStatus,
};
use crate::provider::error::Error;
use crate::provider::retry::Retrying;
use wire::{parse_results, BatchObject, SubmitBody, SubmitResponse};

/// The path of the batches endpoint.
const BATCHES_PATH: &str = "/v1/messages/batches";

/// The path of batch `id`, followed by `suffix` (`""`, `"/results"`,
/// `"/cancel"`).
fn batch_path(id: &BatchId, suffix: &str) -> String {
    format!("{BATCHES_PATH}/{id}{suffix}")
}

/// No body for a call that sends none.
const NO_BODY: Option<&()> = None;

/// One attempt of each batch call against the Messages API, for one model.
#[derive(Clone, Debug)]
pub(super) struct BatchApi {
    api: AnthropicApi,
    model: String,
}

/// Message batches on the Anthropic API, for one model. Every call but
/// `cancel` retries transient failures under the endpoint's retry policy;
/// on Bedrock every call is [`Error::Unsupported`] before any request.
#[derive(Clone, Debug)]
pub struct Batches {
    inner: Retrying<BatchApi>,
}

impl Batches {
    /// Batches for `model` through `api`, retried under its channel's policy.
    pub(super) fn new(api: AnthropicApi, model: String) -> Self {
        let route = api.route().clone();

        Self {
            inner: route.channel().retrying(BatchApi { api, model }),
        }
    }
}

impl BatchApi {
    /// Message batches are not available on Bedrock:
    /// [`Error::Unsupported`] there, before any request.
    fn check_available(&self) -> Result<(), Error> {
        if self.api.route().channel().is_bedrock() {
            return Err(Error::Unsupported(
                "message batches are not available on Bedrock",
            ));
        }

        Ok(())
    }

    /// Sends `method` to `path`, with `body` as JSON when given, and parses
    /// the answer as `T`.
    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + Sync)>,
    ) -> Result<T, Error> {
        self.check_available()?;
        let mut request = self.api.request(method, path).await?;
        if let Some(body) = body {
            request = request.json(body);
        }

        self.api.route().channel().http().json(request).await
    }

    /// Sends a GET to `path` and returns the answer as text.
    async fn get_text(&self, path: &str) -> Result<String, Error> {
        self.check_available()?;
        let request = self.api.request(Method::GET, path).await?;

        self.api.route().channel().http().text(request).await
    }
}

#[async_trait]
impl BatchProvider for Batches {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        self.inner.submit(requests).await
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.status(id).await
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        self.inner.results(id).await
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        self.inner.cancel(id).await
    }
}

#[async_trait]
impl BatchProvider for BatchApi {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        let body = SubmitBody::new(&requests, &self.model)?;
        let response: SubmitResponse = self.call(Method::POST, BATCHES_PATH, Some(&body)).await?;
        Ok(BatchId::from(response.id))
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        let path = batch_path(id, "");
        let object: BatchObject = self.call(Method::GET, &path, NO_BODY).await?;
        Ok(object.into())
    }

    /// Reads the batch's status first; results come from `base_url`, never
    /// from a `results_url` in the answer.
    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        if self.status(id).await?.state() != BatchState::Ended {
            return Err(Error::BatchNotEnded(id.clone()));
        }

        parse_results(&self.get_text(&batch_path(id, "/results")).await?)
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        let path = batch_path(id, "/cancel");
        let object: BatchObject = self.call(Method::POST, &path, NO_BODY).await?;
        Ok(object.into())
    }
}

#[cfg(test)]
mod tests;
