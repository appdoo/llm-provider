//! Message batches on the Messages API: [`BatchProvider`] for the transport
//! (`/v1/messages/batches`) and for [`Anthropic`], which adds retry.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::provider::anthropic::client::{Anthropic, AnthropicApi};
use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::response::{ResponseMessage, WireError};
use crate::provider::batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
use crate::provider::error::{Error, RequestError};

/// The path of the batches endpoint.
const BATCHES_PATH: &str = "/v1/messages/batches";

/// The body of a batch submit.
#[derive(Debug, Serialize)]
struct SubmitBody<'a> {
    requests: Vec<BatchEntry<'a>>,
}

impl<'a> SubmitBody<'a> {
    /// The body for `requests`; an empty batch, a repeated `custom_id` or an
    /// invalid breakpoint layout is a [`RequestError`], checked in that order.
    fn new(requests: &'a [BatchRequest], default_model: &'a str) -> Result<Self, RequestError> {
        todo!()
    }
}

/// One request of a batch submit.
#[derive(Debug, Serialize)]
struct BatchEntry<'a> {
    custom_id: &'a str,
    params: MessagesBody<'a>,
}

/// The answer to a submit.
#[derive(Debug, Deserialize)]
struct SubmitResponse {
    id: String,
}

/// A message batch object, as status, cancel and the results lookup return it.
#[derive(Debug, Deserialize)]
struct BatchObject {
    processing_status: WireState,
    request_counts: RequestCounts,
}

impl From<BatchObject> for BatchStatus {
    fn from(object: BatchObject) -> Self {
        todo!()
    }
}

/// A batch's `processing_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireState {
    InProgress,
    Canceling,
    Ended,
}

impl From<WireState> for BatchState {
    fn from(state: WireState) -> Self {
        todo!()
    }
}

/// A batch's `request_counts`.
#[derive(Debug, Deserialize)]
struct RequestCounts {
    processing: u32,
    succeeded: u32,
    errored: u32,
    canceled: u32,
    expired: u32,
}

impl From<RequestCounts> for BatchCounts {
    fn from(counts: RequestCounts) -> Self {
        todo!()
    }
}

/// One JSONL line of a results file. `result` stays untyped here so a
/// result that does not parse becomes [`BatchOutcome::Malformed`] instead of
/// failing the whole file.
#[derive(Debug, Deserialize)]
struct ResultLine {
    custom_id: String,
    result: serde_json::Value,
}

/// A result line's `result`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireResult {
    Succeeded { message: ResponseMessage },
    Errored { error: ErrorEnvelope },
    Canceled,
    Expired,
}

/// The `error` of an errored result: an envelope around the error object.
#[derive(Debug, Deserialize)]
struct ErrorEnvelope {
    error: WireError,
}

/// The outcome a result line's `result` stands for.
fn outcome(result: serde_json::Value) -> BatchOutcome {
    todo!()
}

/// The results file, in file order, skipping blank lines; a line whose
/// `custom_id` cannot be read fails the whole call.
fn parse_results(body: &str) -> Result<Vec<BatchResult>, Error> {
    todo!()
}

/// The path of batch `id`, followed by `suffix` (`""`, `"/results"`,
/// `"/cancel"`).
fn batch_path(id: &BatchId, suffix: &str) -> String {
    todo!()
}

/// Sends `method` to `path` and reads the batch object it answers with.
async fn batch_status(
    api: &AnthropicApi,
    method: reqwest::Method,
    path: &str,
) -> Result<BatchStatus, Error> {
    todo!()
}

#[async_trait]
impl BatchProvider for AnthropicApi {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        todo!()
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        todo!()
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        todo!()
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        todo!()
    }
}

#[async_trait]
impl BatchProvider for Anthropic {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        todo!()
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        todo!()
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        todo!()
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        todo!()
    }
}
