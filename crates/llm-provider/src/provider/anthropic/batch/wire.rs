//! The wire format of `/v1/messages/batches`: the submit body, the batch
//! object and the JSONL results file.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::provider::anthropic::request::MessagesBody;
use crate::provider::anthropic::response::{ResponseMessage, WireError};
use crate::provider::batch::{
    BatchCounts, BatchOutcome, BatchRequest, BatchResult, BatchState, BatchStatus,
};
use crate::provider::error::{Error, RequestError};

/// The body of a batch submit.
#[derive(Debug, Serialize)]
pub(super) struct SubmitBody<'a> {
    requests: Vec<BatchEntry<'a>>,
}

impl<'a> SubmitBody<'a> {
    /// The body for `requests`; an empty batch, a repeated `custom_id` or an
    /// invalid breakpoint layout is a [`RequestError`], checked in that order.
    pub(super) fn new(
        requests: &'a [BatchRequest],
        default_model: &'a str,
    ) -> Result<Self, RequestError> {
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
    for request in requests {
        if !seen.insert(request.custom_id()) {
            let repeated = request.custom_id().to_string();
            return Err(RequestError::DuplicateCustomId(repeated));
        }
    }

    Ok(())
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
pub(super) struct SubmitResponse {
    pub(super) id: String,
}

/// A message batch object, as status, cancel and the results lookup return it.
#[derive(Debug, Deserialize)]
pub(super) struct BatchObject {
    processing_status: WireState,
    request_counts: RequestCounts,
}

impl From<BatchObject> for BatchStatus {
    fn from(object: BatchObject) -> Self {
        BatchStatus::new(
            object.processing_status.into(),
            object.request_counts.into(),
        )
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
        BatchCounts::new(
            counts.processing,
            counts.succeeded,
            counts.errored,
            counts.canceled,
            counts.expired,
        )
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
pub(super) fn outcome(result: serde_json::Value) -> BatchOutcome {
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
pub(super) fn parse_results(body: &str) -> Result<Vec<BatchResult>, Error> {
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(parse_line)
        .collect()
}

/// One line of a results file as a [`BatchResult`].
fn parse_line(line: &str) -> Result<BatchResult, Error> {
    let line: ResultLine = serde_json::from_str(line)?;
    Ok(BatchResult::new(line.custom_id, outcome(line.result)))
}
