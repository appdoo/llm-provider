//! Message batches on the Messages API: [`BatchProvider`] for the transport
//! (`/v1/messages/batches`). [`Anthropic`](super::Anthropic) adds retry in
//! `client.rs`.

use std::collections::HashSet;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::provider::anthropic::api::AnthropicApi;
use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::response::{ResponseMessage, WireError};
use crate::provider::batch::{
    BatchCounts, BatchId, BatchOutcome, BatchProvider, BatchRequest, BatchResult, BatchState,
    BatchStatus,
};
use crate::provider::error::{Error, RequestError};
use crate::transport::read_text;

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
        if requests.is_empty() {
            return Err(RequestError::EmptyBatch);
        }
        check_unique_ids(requests)?;
        let requests = requests
            .iter()
            .map(|request| BatchEntry::new(request, default_model))
            .collect::<Result<_, _>>()?;
        Ok(Self { requests })
    }
}

/// The first `custom_id` that occurs twice is
/// [`RequestError::DuplicateCustomId`].
fn check_unique_ids(requests: &[BatchRequest]) -> Result<(), RequestError> {
    let mut seen = HashSet::new();
    match requests.iter().find(|r| !seen.insert(r.custom_id())) {
        Some(repeated) => Err(RequestError::DuplicateCustomId(
            repeated.custom_id().to_string(),
        )),
        None => Ok(()),
    }
}

/// One request of a batch submit.
#[derive(Debug, Serialize)]
struct BatchEntry<'a> {
    custom_id: &'a str,
    params: MessagesBody<'a>,
}

impl<'a> BatchEntry<'a> {
    /// The entry for `request`, its body rendered without `stream`.
    fn new(request: &'a BatchRequest, default_model: &'a str) -> Result<Self, RequestError> {
        Ok(Self {
            custom_id: request.custom_id(),
            params: MessagesBody::new(request.request(), default_model)?,
        })
    }
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
        BatchStatus {
            state: object.processing_status.into(),
            counts: object.request_counts.into(),
        }
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
        match state {
            WireState::InProgress => BatchState::InProgress,
            WireState::Canceling => BatchState::Canceling,
            WireState::Ended => BatchState::Ended,
        }
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
        BatchCounts {
            processing: counts.processing,
            succeeded: counts.succeeded,
            errored: counts.errored,
            canceled: counts.canceled,
            expired: counts.expired,
        }
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
    match serde_json::from_value::<WireResult>(result) {
        Ok(WireResult::Succeeded { message }) => BatchOutcome::Succeeded(message.into()),
        Ok(WireResult::Errored { error }) => BatchOutcome::Errored(error.error.into()),
        Ok(WireResult::Canceled) => BatchOutcome::Canceled,
        Ok(WireResult::Expired) => BatchOutcome::Expired,
        Err(e) => BatchOutcome::Malformed(e.to_string()),
    }
}

/// The results file, in file order, skipping blank lines; a line whose
/// `custom_id` cannot be read fails the whole call.
fn parse_results(body: &str) -> Result<Vec<BatchResult>, Error> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_line)
        .collect()
}

/// One line of a results file as a [`BatchResult`].
fn parse_line(line: &str) -> Result<BatchResult, Error> {
    let line: ResultLine = serde_json::from_str(line)?;
    Ok(BatchResult {
        custom_id: line.custom_id,
        outcome: outcome(line.result),
    })
}

/// The path of batch `id`, followed by `suffix` (`""`, `"/results"`,
/// `"/cancel"`).
fn batch_path(id: &BatchId, suffix: &str) -> String {
    format!("{BATCHES_PATH}/{id}{suffix}")
}

/// Sends `method` to `path` and reads the batch object it answers with.
async fn batch_status(
    api: &AnthropicApi,
    method: reqwest::Method,
    path: &str,
) -> Result<BatchStatus, Error> {
    let request = api.request(method, path).await?;
    let object: BatchObject = api.http().json(request).await?;
    Ok(object.into())
}

#[async_trait]
impl BatchProvider for AnthropicApi {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, Error> {
        let body = SubmitBody::new(&requests, self.model())?;
        let request = self
            .request(reqwest::Method::POST, BATCHES_PATH)
            .await?
            .json(&body);
        let response: SubmitResponse = self.http().json(request).await?;
        Ok(BatchId::from(response.id))
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        batch_status(self, reqwest::Method::GET, &batch_path(id, "")).await
    }

    /// Reads the batch's status first; results come from `base_url`, never
    /// from a `results_url` in the answer.
    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, Error> {
        let status = self.status(id).await?;
        if status.state() != BatchState::Ended {
            return Err(Error::BatchNotEnded(id.clone()));
        }
        let path = batch_path(id, "/results");
        let request = self.request(reqwest::Method::GET, &path).await?;
        let response = self.http().send(request).await?;
        let body = read_text(response, self.http().idle_gap()).await?;
        parse_results(&body)
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, Error> {
        batch_status(self, reqwest::Method::POST, &batch_path(id, "/cancel")).await
    }
}

#[cfg(test)]
mod tests;
