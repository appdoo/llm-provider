//! Tests for message batches on the Messages API: submit validation, status,
//! results, cancel, result decoding and retry through [`RetryingProvider`].

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::provider::anthropic::test_support::test_api;
use crate::provider::error::ErrorKind;
use crate::provider::retry::{RetryPolicy, RetryingProvider};
use crate::provider::test_stub::{RecordingSleeper, StubBody, StubReply, StubServer};
use crate::provider::StopReason;
use crate::provider::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, Content, Message,
    MessageRole, ModelSelection, ProviderOptions, ToolUseCall, Usage,
};

const STATUS_IN_PROGRESS: &str =
    include_str!("../../../../tests/fixtures/batch_status_in_progress.json");
const RESULTS: &str = include_str!("../../../../tests/fixtures/batch_results.jsonl");
const RESULTS_BAD: &str = include_str!("../../../../tests/fixtures/batch_results_bad.jsonl");
const RESULTS_PATH: &str = "/v1/messages/batches/msgbatch_1/results";

/// One attempt against the API at `url` with the default options.
fn provider(url: &str) -> AnthropicApi {
    test_api(url, ProviderOptions::default())
}

fn completion_request() -> CompletionRequest {
    CompletionRequest {
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
        response_schema: None,
    }
}

fn batch_request(custom_id: &str) -> BatchRequest {
    BatchRequest::new(custom_id, completion_request()).expect("valid custom_id")
}

fn batch_id(id: &str) -> BatchId {
    BatchId::from(id.to_string())
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
        ok_json(r#"{"id":"msgbatch_1","type":"message_batch","processing_status":"in_progress"}"#),
    )])
    .await;
    let id = provider(&stub.url())
        .submit(vec![batch_request("yt_abc"), batch_request("ig_xyz")])
        .await
        .expect("submit");

    assert_eq!(id, batch_id("msgbatch_1"));

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
    assert_eq!(body["requests"][0]["params"]["model"], "claude-sonnet-5-5");
    assert_eq!(body["requests"][1]["custom_id"], "ig_xyz");
    assert!(body["requests"][1]["params"].get("stream").is_none());
}

/// An invalid `custom_id` is rejected when the request is built, so no
/// submit can carry it.
#[test]
fn invalid_custom_id_fails_before_connecting() {
    let err = BatchRequest::new("a/b", completion_request()).expect_err("must reject");

    assert_eq!(err, RequestError::InvalidCustomId("a/b".to_string()));
    assert_eq!(err.to_string(), "invalid custom_id \"a/b\"");
}

/// The remaining submit validation rows, in table order.
#[test]
fn submit_validation_table() {
    let msg = |reqs: Vec<BatchRequest>| match SubmitBody::new(&reqs, "m") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected RequestError"),
    };
    let id_msg = |id: &str| match BatchRequest::new(id, completion_request()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected InvalidCustomId"),
    };

    assert_eq!(msg(vec![]), "batch has no requests");
    assert_eq!(id_msg(""), "invalid custom_id \"\"");
    assert_eq!(
        id_msg(&"a".repeat(65)),
        format!("invalid custom_id {:?}", "a".repeat(65))
    );
    assert_eq!(id_msg("ü"), "invalid custom_id \"ü\"");
    assert_eq!(
        msg(vec![batch_request("yt_abc"), batch_request("yt_abc")]),
        "duplicate custom_id \"yt_abc\""
    );
    let mut too_many = completion_request();
    too_many.cache_breakpoints = vec![
        CacheBreakpoint {
            position: BreakpointPosition::Message(0),
            ttl: CacheTtl::FiveMinutes,
        };
        5
    ];
    let too_many = BatchRequest::new("ok", too_many).expect("valid custom_id");
    assert_eq!(msg(vec![too_many]), "too many cache breakpoints: 5 (max 4)");
    let ok = [batch_request(&"A-z_9".repeat(12))];
    assert!(SubmitBody::new(&ok, "m").is_ok());
}

/// Status maps `processing_status` and `request_counts`.
#[tokio::test]
async fn status_maps_counts() {
    let stub = StubServer::start(vec![(
        ("GET", "/v1/messages/batches/msgbatch_1"),
        ok_json(STATUS_IN_PROGRESS),
    )])
    .await;
    let status = provider(&stub.url())
        .status(&batch_id("msgbatch_1"))
        .await
        .expect("status");

    assert_eq!(
        status,
        BatchStatus::new(BatchState::InProgress, BatchCounts::new(1, 1, 0, 0, 0))
    );
}

/// An unknown `processing_status` is a serde error → `Json`.
#[tokio::test]
async fn unknown_processing_status_is_json_error() {
    let body = STATUS_IN_PROGRESS.replace("in_progress", "paused");
    let stub = StubServer::start(vec![(
        ("GET", "/v1/messages/batches/msgbatch_1"),
        (200, "application/json", body),
    )])
    .await;
    let err = provider(&stub.url())
        .status(&batch_id("msgbatch_1"))
        .await
        .expect_err("unknown state");

    assert!(matches!(err, Error::Json(_)), "got {err:?}");
}

/// A non-2xx answer becomes `Api` with status, body and kind.
#[tokio::test]
async fn non_2xx_maps_to_api_error() {
    let stub = StubServer::start(vec![]).await;
    let err = provider(&stub.url())
        .status(&batch_id("msgbatch_1"))
        .await
        .expect_err("404");
    match err {
        Error::Api(e) => {
            assert_eq!(e.status(), Some(404));
            assert_eq!(e.message(), "no stub route");
            assert_eq!(e.kind(), ErrorKind::InvalidRequest);
        }
        other => panic!("expected Api, got {other:?}"),
    }
}

/// Results in file order; errored and malformed results typed.
#[tokio::test]
async fn results_in_order_errored_and_malformed() {
    let stub = ended_batch(RESULTS).await;
    let results = provider(&stub.url())
        .results(&batch_id("msgbatch_1"))
        .await
        .expect("results");

    assert_eq!(results.len(), 3);

    assert_eq!(results[0].custom_id(), "yt_abc");
    match results[0].outcome() {
        BatchOutcome::Succeeded(c) => {
            assert_eq!(c.text, "Hi");
            assert_eq!(c.stop, StopReason::EndTurn);
            assert_eq!(c.usage.input_tokens, 10);
            assert_eq!(c.usage.output_tokens, 2);
            assert_eq!(c.usage.cache_read_input_tokens, 0);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }

    assert_eq!(results[1].custom_id(), "ig_xyz");
    let BatchOutcome::Errored(error) = results[1].outcome() else {
        panic!("expected Errored, got {:?}", results[1].outcome());
    };
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.error_type(), Some("invalid_request_error"));
    assert_eq!(error.message(), "max_tokens: Field required");

    assert_eq!(results[2].custom_id(), "yt_def");
    match results[2].outcome() {
        BatchOutcome::Malformed(m) => assert_ne!(m, ""),
        other => panic!("expected Malformed, got {other:?}"),
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

/// A line without a readable `custom_id` fails the call.
#[tokio::test]
async fn unreadable_line_fails_with_json_error() {
    let stub = ended_batch(RESULTS_BAD).await;
    let err = provider(&stub.url())
        .results(&batch_id("msgbatch_1"))
        .await
        .expect_err("bad line");

    assert!(matches!(err, Error::Json(_)), "got {err:?}");
}

/// Results on a batch still in progress: `BatchNotEnded`, one request.
#[tokio::test]
async fn results_before_ended_is_invalid_request() {
    let stub = StubServer::start(vec![(
        ("GET", "/v1/messages/batches/msgbatch_1"),
        ok_json(STATUS_IN_PROGRESS),
    )])
    .await;
    let err = provider(&stub.url())
        .results(&batch_id("msgbatch_1"))
        .await
        .expect_err("not ended");

    assert!(matches!(err, Error::BatchNotEnded(_)), "got {err:?}");
    assert_eq!(err.to_string(), "batch msgbatch_1 has not ended");
    assert_eq!(err.kind(), ErrorKind::InvalidRequest);
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
        .cancel(&batch_id("msgbatch_1"))
        .await
        .expect("cancel");

    assert_eq!(status.state(), BatchState::Canceling);
    assert_eq!(status.counts().processing(), 1);
    assert_eq!(status.counts().succeeded(), 1);
}

/// Message decoding: `tool_use` blocks, `stop_reason` mapping, usage defaults.
#[test]
fn message_decoding() {
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
    // An errored result without an error object reads as Malformed with
    // serde's message.
    assert!(matches!(
        outcome(serde_json::json!({"type":"errored","error":{"type":"error"}})),
        BatchOutcome::Malformed(m) if !m.is_empty()
    ));
}

// --- batch calls through RetryingProvider ---

fn retrying(
    url: &str,
    options: ProviderOptions,
) -> (RetryingProvider<AnthropicApi>, Arc<RecordingSleeper>) {
    let recorder = Arc::new(RecordingSleeper::default());
    let provider = RetryingProvider::new(test_api(url, options), RetryPolicy::default())
        .with_sleeper(recorder.clone());
    (provider, recorder)
}

/// Status retries a 503.
#[tokio::test]
async fn status_retries_server_error() {
    let stub = StubServer::start_replies(vec![(
        ("GET", "/v1/messages/batches/msgbatch_1"),
        vec![
            StubReply::full(503, "text/plain", "unavailable"),
            StubReply::full(200, "application/json", STATUS_IN_PROGRESS),
        ],
    )])
    .await;
    let (provider, _recorder) = retrying(&stub.url(), ProviderOptions::default());
    let status = provider
        .status(&batch_id("msgbatch_1"))
        .await
        .expect("status");

    assert_eq!(status.state(), BatchState::InProgress);
    assert_eq!(stub.requests().len(), 2);
}

/// Submit retries an API rejection (529).
#[tokio::test]
async fn submit_retries_api_rejection() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages/batches"),
        vec![
            StubReply::full(
                529,
                "application/json",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
            StubReply::full(200, "application/json", r#"{"id":"msgbatch_1"}"#),
        ],
    )])
    .await;
    let (provider, _recorder) = retrying(&stub.url(), ProviderOptions::default());
    let id = provider
        .submit(vec![batch_request("yt_abc")])
        .await
        .expect("submit");

    assert_eq!(id, batch_id("msgbatch_1"));
    assert_eq!(stub.requests().len(), 2);
}

/// Submit never retries a timeout: a batch may exist. Real time (1 s):
/// paused time advanced before the stub read the request.
#[tokio::test]
async fn submit_does_not_retry_timeout() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages/batches"),
        vec![StubReply {
            status: 200,
            content_type: "application/json",
            headers: Vec::new(),
            body: StubBody::Hang,
        }],
    )])
    .await;
    let options = ProviderOptions::default()
        .connect_timeout(Duration::from_secs(30))
        .idle_gap(Duration::from_secs(1));
    let (provider, recorder) = retrying(&stub.url(), options);
    let started = std::time::Instant::now();
    let err = provider
        .submit(vec![batch_request("yt_abc")])
        .await
        .expect_err("must time out");

    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(
        matches!(err, Error::IdleTimeout { after } if after == Duration::from_secs(1)),
        "got {err:?}"
    );
    assert_eq!(stub.requests().len(), 1);
    assert_eq!(recorder.slept(), Vec::<Duration>::new());
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
        .results(&batch_id("b1"))
        .await
        .expect("results");

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].custom_id(), "yt_abc");
    let rec = a.requests();
    assert_eq!(rec.len(), 2);
    assert_eq!(rec[1].path, "/v1/messages/batches/b1/results");
    assert_eq!(
        rec[1].headers.get("x-api-key").map(String::as_str),
        Some("test-key")
    );
    assert_eq!(b.requests().len(), 0);
}

/// Every batch operation of a Bedrock client is unsupported and sends no
/// request.
#[tokio::test]
async fn bedrock_batches_are_unsupported_without_request() {
    let stub = StubServer::start(vec![]).await;
    let api = crate::provider::anthropic::builder::AnthropicBuilder::new(
        "ABSK-test".into(),
        "eu.anthropic.claude-haiku-5-5".to_string(),
    )
    .bedrock("eu-central-1")
    .base_url(stub.url())
    .api();
    let unsupported = |result: Result<(), Error>| match result {
        Err(Error::Unsupported(what)) => {
            assert_eq!(what, "message batches are not available on Bedrock");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    };

    unsupported(api.submit(vec![batch_request("a")]).await.map(drop));
    unsupported(api.status(&batch_id("msgbatch_1")).await.map(drop));
    unsupported(api.results(&batch_id("msgbatch_1")).await.map(drop));
    unsupported(api.cancel(&batch_id("msgbatch_1")).await.map(drop));
    assert_eq!(stub.requests().len(), 0);
}
