//! Message Batches: submit many `CompletionRequest`s as one batch and collect
//! per-request results. `AnthropicProvider` implements it against
//! `/v1/messages/batches`. The provider never polls; that is the caller's loop.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::provider::anthropic::{
    read_capped, validate_breakpoints, AnthropicProvider, ERROR_BODY_CAP,
};
use crate::provider::error::{ApiError, ProviderError};
use crate::provider::event::StopReason;
use crate::provider::sse::anthropic_stop_reason;
use crate::provider::traits::{Completion, CompletionRequest, ToolUseCall, Usage};

/// A provider that can run requests as an asynchronous batch.
#[async_trait]
pub trait BatchProvider: Send + Sync {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, ProviderError>;
    async fn status(&self, id: &BatchId) -> Result<BatchStatus, ProviderError>;
    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, ProviderError>;
    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, ProviderError>;
}

/// One request in a batch; `custom_id` must match `^[a-zA-Z0-9_-]{1,64}$`.
#[derive(Debug, Clone)]
pub struct BatchRequest {
    pub custom_id: String,
    pub request: CompletionRequest,
}

/// Server-assigned batch identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchId(pub String);

/// Processing state and per-outcome request counts of a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchStatus {
    pub state: BatchState,
    pub succeeded: u32,
    pub errored: u32,
    pub canceled: u32,
    pub expired: u32,
    pub processing: u32,
}

/// A batch's `processing_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BatchState {
    InProgress,
    Canceling,
    Ended,
}

/// The outcome of one request, keyed by its `custom_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchResult {
    pub custom_id: String,
    pub outcome: BatchOutcome,
}

/// What happened to one batch request.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BatchOutcome {
    Succeeded(Completion),
    /// The provider rejected this request.
    Errored(ApiError),
    /// The result could not be read. The text is the parser's message.
    Malformed(String),
    Canceled,
    Expired,
}

/// Wire shape of a message batch object (status, cancel, results lookup).
#[derive(Deserialize)]
struct BatchBody {
    processing_status: BatchState,
    request_counts: RequestCounts,
}

#[derive(Deserialize)]
struct RequestCounts {
    processing: u32,
    succeeded: u32,
    errored: u32,
    canceled: u32,
    expired: u32,
}

impl From<BatchBody> for BatchStatus {
    fn from(body: BatchBody) -> Self {
        let c = body.request_counts;
        BatchStatus {
            state: body.processing_status,
            succeeded: c.succeeded,
            errored: c.errored,
            canceled: c.canceled,
            expired: c.expired,
            processing: c.processing,
        }
    }
}

#[derive(Deserialize)]
struct SubmitResponse {
    id: String,
}

/// One JSONL line of a results file.
#[derive(Deserialize)]
struct ResultLine {
    custom_id: String,
    result: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireResult {
    Succeeded { message: WireMessage },
    Errored { error: serde_json::Value },
    Canceled,
    Expired,
}

#[derive(Deserialize)]
struct WireMessage {
    /// Blocks stay untyped so unknown block types (e.g. `thinking`) are skipped.
    content: Vec<serde_json::Value>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize, Default)]
struct WireUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

/// `^[a-zA-Z0-9_-]{1,64}$`, checked by hand.
fn valid_custom_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Local validation, in a fixed order, before any HTTP call.
fn validate_batch(requests: &[BatchRequest]) -> Result<(), ProviderError> {
    if requests.is_empty() {
        return Err(ProviderError::InvalidRequest(
            "batch has no requests".to_string(),
        ));
    }
    if let Some(bad) = requests.iter().find(|r| !valid_custom_id(&r.custom_id)) {
        return Err(ProviderError::InvalidRequest(format!(
            "invalid custom_id {:?}",
            bad.custom_id
        )));
    }
    let mut seen = HashSet::new();
    for r in requests {
        if !seen.insert(r.custom_id.as_str()) {
            return Err(ProviderError::InvalidRequest(format!(
                "duplicate custom_id {:?}",
                r.custom_id
            )));
        }
    }
    for r in requests {
        validate_breakpoints(&r.request)?;
    }
    Ok(())
}

fn to_completion(message: WireMessage) -> Completion {
    let mut text = String::new();
    let mut tool_uses = Vec::new();
    for block in message.content {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(t) = block["text"].as_str() {
                    text.push_str(t);
                }
            }
            Some("tool_use") => tool_uses.push(ToolUseCall {
                id: block["id"].as_str().unwrap_or_default().to_string(),
                name: block["name"].as_str().unwrap_or_default().to_string(),
                input: block["input"].clone(),
            }),
            _ => {}
        }
    }
    let stop = message
        .stop_reason
        .as_deref()
        .map_or(StopReason::EndTurn, anthropic_stop_reason);
    let u = message.usage;
    Completion {
        text,
        tool_uses,
        usage: Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_creation_input_tokens: u.cache_creation_input_tokens,
            cache_read_input_tokens: u.cache_read_input_tokens,
        },
        stop,
    }
}

fn to_outcome(result: serde_json::Value) -> BatchOutcome {
    match serde_json::from_value::<WireResult>(result) {
        Ok(WireResult::Succeeded { message }) => BatchOutcome::Succeeded(to_completion(message)),
        Ok(WireResult::Errored { error }) if error["error"].is_object() => {
            BatchOutcome::Errored(ApiError::from_stream_event(&error, true))
        }
        Ok(WireResult::Errored { .. }) => {
            BatchOutcome::Malformed("errored result has no error object".to_string())
        }
        Ok(WireResult::Canceled) => BatchOutcome::Canceled,
        Ok(WireResult::Expired) => BatchOutcome::Expired,
        Err(e) => BatchOutcome::Malformed(e.to_string()),
    }
}

/// Parse a JSONL results file in file order, skipping blank lines. A line
/// whose `custom_id` cannot be read fails the whole call.
fn parse_results(body: &str) -> Result<Vec<BatchResult>, ProviderError> {
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let parsed: ResultLine = serde_json::from_str(line)?;
            Ok(BatchResult {
                custom_id: parsed.custom_id,
                outcome: to_outcome(parsed.result),
            })
        })
        .collect()
}

/// Run `fut`, failing with `IdleTimeout { after: gap }` when it takes longer.
async fn within<T, E>(
    gap: Duration,
    fut: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, ProviderError>
where
    ProviderError: From<E>,
{
    match tokio::time::timeout(gap, fut).await {
        Ok(result) => Ok(result?),
        Err(_) => Err(ProviderError::IdleTimeout { after: gap }),
    }
}

/// Read a response body within `gap`.
async fn read_text(response: reqwest::Response, gap: Duration) -> Result<String, ProviderError> {
    within(gap, response.text()).await
}

/// Send within `gap` and turn a non-2xx answer into
/// `Api(ApiError::from_response(..))`.
async fn send_checked(
    req: reqwest::RequestBuilder,
    gap: Duration,
) -> Result<reqwest::Response, ProviderError> {
    let response = within(gap, req.send()).await?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_capped(response, ERROR_BODY_CAP, gap).await?;
        return Err(ProviderError::Api(ApiError::from_response(
            status, &headers, &body,
        )));
    }
    Ok(response)
}

impl AnthropicProvider {
    fn batches_url(&self) -> String {
        format!("{}/v1/messages/batches", self.base_url())
    }

    async fn batch_body(
        &self,
        method: reqwest::Method,
        url: &str,
    ) -> Result<BatchBody, ProviderError> {
        let response = send_checked(self.request(method, url), self.idle_gap()).await?;
        let text = read_text(response, self.idle_gap()).await?;
        Ok(serde_json::from_str(&text)?)
    }
}

#[async_trait]
impl BatchProvider for AnthropicProvider {
    async fn submit(&self, requests: Vec<BatchRequest>) -> Result<BatchId, ProviderError> {
        validate_batch(&requests)?;
        let entries: Vec<serde_json::Value> = requests
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "custom_id": r.custom_id,
                    "params": self.build_request_body(&r.request),
                })
            })
            .collect();
        let body = serde_json::json!({ "requests": entries });
        let response = send_checked(
            self.request(reqwest::Method::POST, &self.batches_url())
                .json(&body),
            self.idle_gap(),
        )
        .await?;
        let text = read_text(response, self.idle_gap()).await?;
        let parsed: SubmitResponse = serde_json::from_str(&text)?;
        Ok(BatchId(parsed.id))
    }

    async fn status(&self, id: &BatchId) -> Result<BatchStatus, ProviderError> {
        let url = format!("{}/{}", self.batches_url(), id.0);
        Ok(self.batch_body(reqwest::Method::GET, &url).await?.into())
    }

    async fn results(&self, id: &BatchId) -> Result<Vec<BatchResult>, ProviderError> {
        let url = format!("{}/{}", self.batches_url(), id.0);
        let body = self.batch_body(reqwest::Method::GET, &url).await?;
        if body.processing_status != BatchState::Ended {
            return Err(ProviderError::InvalidRequest(format!(
                "batch {} has not ended",
                id.0
            )));
        }
        let results_url = format!("{}/{}/results", self.batches_url(), id.0);
        let response = send_checked(
            self.request(reqwest::Method::GET, &results_url),
            self.idle_gap(),
        )
        .await?;
        let text = read_text(response, self.idle_gap()).await?;
        parse_results(&text)
    }

    async fn cancel(&self, id: &BatchId) -> Result<BatchStatus, ProviderError> {
        let url = format!("{}/{}/cancel", self.batches_url(), id.0);
        Ok(self.batch_body(reqwest::Method::POST, &url).await?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::auth::AuthMethod;
    use crate::provider::error::ErrorKind;
    use crate::provider::test_stub::StubServer;
    use crate::provider::traits::{Content, Message, MessageRole, ModelSelection};
    use std::sync::Arc;

    const STATUS_IN_PROGRESS: &str =
        include_str!("../../tests/fixtures/batch_status_in_progress.json");
    const RESULTS: &str = include_str!("../../tests/fixtures/batch_results.jsonl");
    const RESULTS_BAD: &str = include_str!("../../tests/fixtures/batch_results_bad.jsonl");
    const RESULTS_PATH: &str = "/v1/messages/batches/msgbatch_1/results";

    fn provider(url: &str) -> AnthropicProvider {
        AnthropicProvider::with_base_url(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".into()),
            url,
        )
    }

    fn batch_request(custom_id: &str) -> BatchRequest {
        BatchRequest {
            custom_id: custom_id.to_string(),
            request: CompletionRequest {
                model: ModelSelection::Default,
                system: None,
                messages: Arc::from(vec![Message {
                    role: MessageRole::User,
                    content: vec![Content::Text {
                        text: "summarize".to_string(),
                    }],
                }]),
                tools: vec![],
                max_tokens: 64,
                cache_breakpoints: Vec::new(),
            },
        }
    }

    fn ok_json(body: &str) -> (u16, &'static str, String) {
        (200, "application/json", body.to_string())
    }

    /// Starts one stub serving the `ended` status body and the results `jsonl`.
    async fn ended_batch(jsonl: &str) -> StubServer {
        let ended = serde_json::json!({
            "id": "msgbatch_1",
            "processing_status": "ended",
            "request_counts": {"processing": 0, "succeeded": 1, "errored": 2, "canceled": 0, "expired": 0},
        });
        StubServer::start(vec![
            (
                ("GET", "/v1/messages/batches/msgbatch_1"),
                ok_json(&ended.to_string()),
            ),
            (
                ("GET", RESULTS_PATH),
                (200, "application/x-jsonl", jsonl.to_string()),
            ),
        ])
        .await
    }

    /// Submit posts both requests with params that carry no `stream` key and
    /// returns the server's batch id.
    #[tokio::test]
    async fn submit_posts_requests_and_returns_id() {
        let stub = StubServer::start(vec![(
            ("POST", "/v1/messages/batches"),
            ok_json(
                r#"{"id":"msgbatch_1","type":"message_batch","processing_status":"in_progress"}"#,
            ),
        )])
        .await;
        let id = provider(&stub.url())
            .submit(vec![batch_request("yt_abc"), batch_request("ig_xyz")])
            .await
            .expect("submit");
        assert_eq!(id, BatchId("msgbatch_1".to_string()));

        let requests = stub.requests();
        assert_eq!(requests.len(), 1);
        let rec = &requests[0];
        assert_eq!(
            rec.headers.get("x-api-key").map(String::as_str),
            Some("test-key")
        );
        assert_eq!(
            rec.headers.get("anthropic-version").map(String::as_str),
            Some("2023-06-01")
        );
        assert!(!rec.headers.contains_key("anthropic-beta"));
        let body: serde_json::Value = serde_json::from_str(&rec.body).expect("json body");
        assert_eq!(body["requests"][0]["custom_id"], "yt_abc");
        assert!(body["requests"][0]["params"].get("stream").is_none());
        assert_eq!(body["requests"][0]["params"]["model"], "claude-sonnet-4-6");
        assert_eq!(body["requests"][1]["custom_id"], "ig_xyz");
        assert!(body["requests"][1]["params"].get("stream").is_none());
    }

    /// An invalid custom_id is rejected before any connection.
    #[tokio::test]
    async fn invalid_custom_id_fails_before_connecting() {
        let stub = StubServer::start(vec![]).await;
        let err = provider(&stub.url())
            .submit(vec![batch_request("a/b")])
            .await
            .expect_err("must reject");
        match err {
            ProviderError::InvalidRequest(msg) => assert_eq!(msg, "invalid custom_id \"a/b\""),
            other => panic!("expected InvalidRequest, got {:?}", other),
        }
        assert_eq!(stub.connection_count(), 0);
    }

    /// The remaining submit validation rows, in table order.
    #[test]
    fn submit_validation_table() {
        let msg = |reqs: Vec<BatchRequest>| match validate_batch(&reqs) {
            Err(ProviderError::InvalidRequest(m)) => m,
            other => panic!("expected InvalidRequest, got {:?}", other),
        };
        assert_eq!(msg(vec![]), "batch has no requests");
        assert_eq!(msg(vec![batch_request("")]), "invalid custom_id \"\"");
        assert_eq!(
            msg(vec![batch_request(&"a".repeat(65))]),
            format!("invalid custom_id {:?}", "a".repeat(65))
        );
        assert_eq!(msg(vec![batch_request("ü")]), "invalid custom_id \"ü\"");
        assert_eq!(
            msg(vec![batch_request("yt_abc"), batch_request("yt_abc")]),
            "duplicate custom_id \"yt_abc\""
        );
        let mut too_many = batch_request("ok");
        too_many.request.cache_breakpoints = vec![
            crate::provider::traits::CacheBreakpoint {
                position: crate::provider::traits::BreakpointPosition::Message(0),
                ttl: crate::provider::traits::CacheTtl::FiveMinutes,
            };
            5
        ];
        assert_eq!(msg(vec![too_many]), "too many cache breakpoints: 5 (max 4)");
        assert!(validate_batch(&[batch_request(&"A-z_9".repeat(12))]).is_ok());
    }

    /// Status maps processing_status and request_counts.
    #[tokio::test]
    async fn status_maps_counts() {
        let stub = StubServer::start(vec![(
            ("GET", "/v1/messages/batches/msgbatch_1"),
            ok_json(STATUS_IN_PROGRESS),
        )])
        .await;
        let status = provider(&stub.url())
            .status(&BatchId("msgbatch_1".to_string()))
            .await
            .expect("status");
        assert_eq!(
            status,
            BatchStatus {
                state: BatchState::InProgress,
                processing: 1,
                succeeded: 1,
                errored: 0,
                canceled: 0,
                expired: 0,
            }
        );
    }

    /// An unknown processing_status is a serde error → `Json`.
    #[tokio::test]
    async fn unknown_processing_status_is_json_error() {
        let body = STATUS_IN_PROGRESS.replace("in_progress", "paused");
        let stub = StubServer::start(vec![(
            ("GET", "/v1/messages/batches/msgbatch_1"),
            (200, "application/json", body),
        )])
        .await;
        let err = provider(&stub.url())
            .status(&BatchId("msgbatch_1".to_string()))
            .await
            .expect_err("unknown state");
        assert!(matches!(err, ProviderError::Json(_)), "got {:?}", err);
    }

    /// A non-2xx answer becomes `Api` with status, body and kind.
    #[tokio::test]
    async fn non_2xx_maps_to_api_error() {
        let stub = StubServer::start(vec![]).await;
        let err = provider(&stub.url())
            .status(&BatchId("msgbatch_1".to_string()))
            .await
            .expect_err("404");
        match err {
            ProviderError::Api(e) => {
                assert_eq!(e.status, Some(404));
                assert_eq!(e.message, "no stub route");
                assert_eq!(e.kind, crate::provider::error::ErrorKind::InvalidRequest);
            }
            other => panic!("expected Api, got {:?}", other),
        }
    }

    /// Results in file order; errored and malformed results typed.
    #[tokio::test]
    async fn results_in_order_errored_and_malformed() {
        let stub = ended_batch(RESULTS).await;
        let results = provider(&stub.url())
            .results(&BatchId("msgbatch_1".to_string()))
            .await
            .expect("results");
        assert_eq!(results.len(), 3);

        assert_eq!(results[0].custom_id, "yt_abc");
        match &results[0].outcome {
            BatchOutcome::Succeeded(c) => {
                assert_eq!(c.text, "Hi");
                assert_eq!(c.stop, StopReason::EndTurn);
                assert_eq!(c.usage.input_tokens, 10);
                assert_eq!(c.usage.output_tokens, 2);
                assert_eq!(c.usage.cache_read_input_tokens, 0);
            }
            other => panic!("expected Succeeded, got {:?}", other),
        }

        assert_eq!(results[1].custom_id, "ig_xyz");
        assert_eq!(
            results[1].outcome,
            BatchOutcome::Errored(ApiError {
                kind: ErrorKind::InvalidRequest,
                status: None,
                message: "max_tokens: Field required".into(),
                request_id: None,
                retry_after: None,
            })
        );

        assert_eq!(results[2].custom_id, "yt_def");
        match &results[2].outcome {
            BatchOutcome::Malformed(m) => assert!(!m.is_empty()),
            other => panic!("expected Malformed, got {:?}", other),
        }

        // The results file was fetched from the documented path on base_url, with auth.
        let rec = stub.requests();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[1].path, RESULTS_PATH);
        assert_eq!(
            rec[1].headers.get("x-api-key").map(String::as_str),
            Some("test-key")
        );
    }

    /// A line without a readable custom_id fails the call.
    #[tokio::test]
    async fn unreadable_line_fails_with_json_error() {
        let stub = ended_batch(RESULTS_BAD).await;
        let err = provider(&stub.url())
            .results(&BatchId("msgbatch_1".to_string()))
            .await
            .expect_err("bad line");
        assert!(matches!(err, ProviderError::Json(_)), "got {:?}", err);
    }

    /// Results on a batch still in progress: InvalidRequest, one request.
    #[tokio::test]
    async fn results_before_ended_is_invalid_request() {
        let stub = StubServer::start(vec![(
            ("GET", "/v1/messages/batches/msgbatch_1"),
            ok_json(STATUS_IN_PROGRESS),
        )])
        .await;
        let err = provider(&stub.url())
            .results(&BatchId("msgbatch_1".to_string()))
            .await
            .expect_err("not ended");
        match err {
            ProviderError::InvalidRequest(msg) => assert_eq!(msg, "batch msgbatch_1 has not ended"),
            other => panic!("expected InvalidRequest, got {:?}", other),
        }
        assert_eq!(stub.requests().len(), 1);
    }

    /// Cancel posts to /cancel and maps the body like status.
    #[tokio::test]
    async fn cancel_maps_canceling() {
        let body = STATUS_IN_PROGRESS.replace("\"in_progress\"", "\"canceling\"");
        let stub = StubServer::start(vec![(
            ("POST", "/v1/messages/batches/msgbatch_1/cancel"),
            (200, "application/json", body),
        )])
        .await;
        let status = provider(&stub.url())
            .cancel(&BatchId("msgbatch_1".to_string()))
            .await
            .expect("cancel");
        assert_eq!(status.state, BatchState::Canceling);
        assert_eq!(status.processing, 1);
        assert_eq!(status.succeeded, 1);
    }

    /// Message decoding: tool_use blocks, stop_reason mapping, usage defaults.
    #[test]
    fn message_decoding() {
        let outcome = |v: serde_json::Value| to_outcome(v);
        let msg = |stop: &str| {
            serde_json::json!({"type": "succeeded", "message": {
                "content": [
                    {"type": "thinking", "thinking": ""},
                    {"type": "text", "text": "A"},
                    {"type": "tool_use", "id": "tu_1", "name": "search", "input": {"q": 1}},
                    {"type": "text", "text": "B"}
                ],
                "stop_reason": stop,
                "usage": {"input_tokens": 3, "cache_creation_input_tokens": 7}
            }})
        };
        let BatchOutcome::Succeeded(c) = outcome(msg("tool_use")) else {
            panic!("expected Succeeded")
        };
        assert_eq!(c.text, "AB");
        assert_eq!(
            c.tool_uses,
            vec![ToolUseCall {
                id: "tu_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({"q": 1}),
            }]
        );
        assert_eq!(c.stop, StopReason::ToolUse);
        assert_eq!(
            c.usage,
            Usage {
                input_tokens: 3,
                output_tokens: 0,
                cache_creation_input_tokens: 7,
                cache_read_input_tokens: 0,
            }
        );
        for (wire, want) in [
            ("end_turn", StopReason::EndTurn),
            ("stop_sequence", StopReason::StopSequence),
            ("max_tokens", StopReason::MaxTokens),
            ("refusal", StopReason::Refusal),
            ("brand_new", StopReason::Unknown),
        ] {
            let BatchOutcome::Succeeded(c) = outcome(msg(wire)) else {
                panic!("expected Succeeded")
            };
            assert_eq!(c.stop, want, "stop_reason {wire}");
        }
        assert_eq!(
            outcome(serde_json::json!({"type": "canceled"})),
            BatchOutcome::Canceled
        );
        assert_eq!(
            outcome(serde_json::json!({"type": "expired"})),
            BatchOutcome::Expired
        );
        assert!(matches!(
            outcome(serde_json::json!({"type": "succeeded"})),
            BatchOutcome::Malformed(_)
        ));
        assert_eq!(
            outcome(serde_json::json!({"type":"errored","error":{"type":"error"}})),
            BatchOutcome::Malformed("errored result has no error object".to_string())
        );
    }

    // --- batch calls through RetryingProvider ---

    fn retrying(
        url: &str,
        options: crate::provider::traits::ProviderOptions,
    ) -> (
        crate::provider::retry::RetryingProvider<AnthropicProvider>,
        Arc<crate::provider::test_stub::RecordingSleeper>,
    ) {
        let recorder = Arc::new(crate::provider::test_stub::RecordingSleeper::default());
        let inner = AnthropicProvider::with_options(
            "claude-sonnet-4-6",
            AuthMethod::ApiKey("test-key".into()),
            url,
            options,
        )
        .expect("provider");
        let provider = crate::provider::retry::RetryingProvider::with_sleeper(
            inner,
            crate::provider::retry::RetryPolicy::default(),
            recorder.clone(),
        );
        (provider, recorder)
    }

    /// Status retries a 503.
    #[tokio::test]
    async fn status_retries_server_error() {
        let stub = StubServer::start_replies(vec![(
            ("GET", "/v1/messages/batches/msgbatch_1"),
            vec![
                crate::provider::test_stub::StubReply::full(503, "text/plain", "unavailable"),
                crate::provider::test_stub::StubReply::full(
                    200,
                    "application/json",
                    STATUS_IN_PROGRESS,
                ),
            ],
        )])
        .await;
        let (provider, _recorder) = retrying(&stub.url(), Default::default());
        let status = provider
            .status(&BatchId("msgbatch_1".to_string()))
            .await
            .expect("status");
        assert_eq!(status.state, BatchState::InProgress);
        assert_eq!(stub.requests().len(), 2);
    }

    /// Submit retries an API rejection (529).
    #[tokio::test]
    async fn submit_retries_api_rejection() {
        let stub = StubServer::start_replies(vec![(
            ("POST", "/v1/messages/batches"),
            vec![
                crate::provider::test_stub::StubReply::full(
                    529,
                    "application/json",
                    r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
                ),
                crate::provider::test_stub::StubReply::full(200, "application/json", r#"{"id":"msgbatch_1"}"#),
            ],
        )])
        .await;
        let (provider, _recorder) = retrying(&stub.url(), Default::default());
        let id = provider
            .submit(vec![batch_request("yt_abc")])
            .await
            .expect("submit");
        assert_eq!(id, BatchId("msgbatch_1".to_string()));
        assert_eq!(stub.requests().len(), 2);
    }

    /// Submit never retries a timeout: a batch may exist. Real
    /// time (1 s): paused time advanced before the stub read the request.
    #[tokio::test]
    async fn submit_does_not_retry_timeout() {
        let stub = StubServer::start_replies(vec![(
            ("POST", "/v1/messages/batches"),
            vec![crate::provider::test_stub::StubReply {
                status: 200,
                content_type: "application/json",
                headers: Vec::new(),
                body: crate::provider::test_stub::StubBody::Hang,
            }],
        )])
        .await;
        let options = crate::provider::traits::ProviderOptions {
            connect_timeout: Duration::from_secs(30),
            idle_gap: Duration::from_secs(1),
        };
        let (provider, recorder) = retrying(&stub.url(), options);
        let started = std::time::Instant::now();
        let err = provider
            .submit(vec![batch_request("yt_abc")])
            .await
            .expect_err("must time out");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(
            matches!(err, ProviderError::IdleTimeout { after } if after == Duration::from_secs(1)),
            "got {err:?}"
        );
        assert_eq!(stub.requests().len(), 1);
        assert!(recorder.slept().is_empty());
    }

    /// A `results_url` in the status body is ignored; results come from `base_url`.
    #[tokio::test]
    async fn results_url_in_body_is_ignored() {
        let b = StubServer::start(vec![(
            ("GET", "/steal"),
            (200, "application/x-jsonl", RESULTS.to_string()),
        )])
        .await;
        let ended = serde_json::json!({
            "id": "b1",
            "processing_status": "ended",
            "request_counts": {"processing": 0, "succeeded": 1, "errored": 2, "canceled": 0, "expired": 0},
            "results_url": format!("{}/steal", b.url()),
        });
        let a = StubServer::start(vec![
            (
                ("GET", "/v1/messages/batches/b1"),
                ok_json(&ended.to_string()),
            ),
            (
                ("GET", "/v1/messages/batches/b1/results"),
                (200, "application/x-jsonl", RESULTS.to_string()),
            ),
        ])
        .await;
        let results = provider(&a.url())
            .results(&BatchId("b1".to_string()))
            .await
            .expect("results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].custom_id, "yt_abc");
        let rec = a.requests();
        assert_eq!(rec.len(), 2);
        assert_eq!(rec[1].path, "/v1/messages/batches/b1/results");
        assert_eq!(
            rec[1].headers.get("x-api-key").map(String::as_str),
            Some("test-key")
        );
        assert_eq!(b.requests().len(), 0);
    }
}
