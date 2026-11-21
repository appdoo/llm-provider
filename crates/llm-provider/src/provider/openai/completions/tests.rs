//! Tests for the raw path on `/v1/completions`: the body, the decoder, stop
//! and usage mapping, the decoder's failures and the checks before any I/O.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio_stream::StreamExt;

use crate::provider::completion::Usage;
use crate::provider::error::{Error, RequestError};
use crate::provider::test_stub::{StubReply, StubServer};
use crate::provider::{
    Bedrock, EventStream, OpenAi, RawLlm, RawRequest, Sampling, StopReason, StreamEvent,
};

/// The recorded llama-server stream (V0-5a) and the events it decodes to.
const LLAMA_SSE: &str = include_str!("../../../../tests/fixtures/raw-llama-server.sse");
const LLAMA_EVENTS: &str = include_str!("../../../../tests/fixtures/raw-llama-server.events.json");

/// The R-1 chunks: `"ab"`, `"c"`, an empty text with finish `stop`.
const AB: &str = r#"{"choices":[{"text":"ab","finish_reason":null}]}"#;
const C: &str = r#"{"choices":[{"text":"c","finish_reason":null}]}"#;
const STOP: &str = r#"{"choices":[{"text":"","finish_reason":"stop"}]}"#;
const DONE: &str = "[DONE]";

/// `chunks` as an event-stream body, one `data:` event each.
fn sse(chunks: &[&str]) -> String {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str("data: ");
        body.push_str(chunk);
        body.push_str("\n\n");
    }
    body
}

/// A stub answering every `POST /v1/completions` with `body`.
async fn stub(body: &str) -> StubServer {
    let route = ("POST", "/v1/completions");
    StubServer::start(vec![(route, (200, "text/event-stream", body.to_string()))]).await
}

/// The model `qwen3-8b` on the stub's `/v1` root.
fn model(stub: &StubServer) -> RawLlm {
    OpenAi::at(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .raw("qwen3-8b")
}

/// Every event of `stream`, which must not fail.
async fn events(stream: EventStream) -> Vec<StreamEvent> {
    stream
        .collect::<Result<Vec<_>, _>>()
        .await
        .expect("stream succeeds")
}

/// The one request body the stub saw, as JSON.
fn sent_body(stub: &StubServer) -> Value {
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    serde_json::from_str(&requests[0].body).expect("json body")
}

/// The request error of a rejected request.
fn request_error(error: Error) -> RequestError {
    match error {
        Error::InvalidRequest(error) => error,
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
}

/// R-1: the prompt goes out byte for byte with model, 4096 tokens, stream
/// and usage on and no chat key; `"ab"`, `"c"` stream as deltas, the empty
/// text as none, then `Stop(EndTurn)`, `MessageStop`.
#[tokio::test]
async fn raw_prompt_streams_deltas_and_body() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let prompt = "<|im_start|>assistant\nfn fib(";

    let events = events(model(&stub).stream(RawRequest::new(prompt))).await;

    let expected = vec![
        StreamEvent::ContentDelta("ab".to_string()),
        StreamEvent::ContentDelta("c".to_string()),
        StreamEvent::Stop(StopReason::EndTurn),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
    let requests = stub.requests();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/completions");
    let body = sent_body(&stub);
    let expected_body = json!({
        "model": "qwen3-8b",
        "prompt": "<|im_start|>assistant\nfn fib(",
        "max_tokens": 4096,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    assert_eq!(body, expected_body);
    assert!(body.get("messages").is_none());
}

/// One event of the fixture's expected list, in serde's external tagging.
#[derive(Deserialize)]
enum RecordedEvent {
    ContentDelta(String),
    Usage(RecordedUsage),
    Stop(String),
    MessageStop,
}

/// The usage of a [`RecordedEvent::Usage`].
#[derive(Deserialize)]
struct RecordedUsage {
    #[serde(rename = "input_tokens")]
    input: u32,
    #[serde(rename = "output_tokens")]
    output: u32,
    #[serde(rename = "cache_creation_input_tokens")]
    cache_creation: u32,
    #[serde(rename = "cache_read_input_tokens")]
    cache_read: u32,
}

impl From<RecordedEvent> for StreamEvent {
    fn from(event: RecordedEvent) -> Self {
        match event {
            RecordedEvent::ContentDelta(text) => StreamEvent::ContentDelta(text),
            RecordedEvent::Usage(usage) => StreamEvent::Usage(Usage {
                input_tokens: usage.input,
                output_tokens: usage.output,
                cache_creation_input_tokens: usage.cache_creation,
                cache_creation_1h_input_tokens: 0,
                cache_read_input_tokens: usage.cache_read,
            }),
            RecordedEvent::Stop(reason) => StreamEvent::Stop(match reason.as_str() {
                "EndTurn" => StopReason::EndTurn,
                "MaxTokens" => StopReason::MaxTokens,
                other => panic!("unexpected stop reason {other}"),
            }),
            RecordedEvent::MessageStop => StreamEvent::MessageStop,
        }
    }
}

/// R-2: the recorded llama-server stream decodes to its expected events.
#[tokio::test]
async fn llama_server_fixture_decodes_to_recorded_events() {
    let stub = stub(LLAMA_SSE).await;
    let recorded: Vec<RecordedEvent> = serde_json::from_str(LLAMA_EVENTS).expect("events json");
    let expected: Vec<StreamEvent> = recorded.into_iter().map(StreamEvent::from).collect();

    let events = events(model(&stub).stream(RawRequest::new("hi"))).await;

    assert_eq!(events, expected);
}

/// R-3: a final finish `length` completes to `MaxTokens` with text `"abc"`.
#[tokio::test]
async fn finish_length_is_max_tokens() {
    let length = r#"{"choices":[{"text":"","finish_reason":"length"}]}"#;
    let stub = stub(&sse(&[AB, C, length, DONE])).await;

    let completion = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect("complete");

    assert_eq!(completion.stop, StopReason::MaxTokens);
    assert_eq!(completion.text, "abc");
}

/// R-4: usage 12 prompt tokens, 8 of them cached, and 3 completion tokens
/// is 4 input, 8 cache reads, 3 output.
#[tokio::test]
async fn final_chunk_usage_splits_cached_tokens() {
    let last = r#"{"choices":[{"text":"","finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":8}}}"#;
    let stub = stub(&sse(&[AB, C, last, DONE])).await;

    let completion = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect("complete");

    assert_eq!(completion.usage.input_tokens, 4);
    assert_eq!(completion.usage.cache_read_input_tokens, 8);
    assert_eq!(completion.usage.output_tokens, 3);
}

/// R-4b: a usage-only chunk before `[DONE]` yields `Usage{5, 2}`, then
/// `Stop(EndTurn)` and `MessageStop`.
#[tokio::test]
async fn usage_only_chunk_precedes_stop() {
    let usage = r#"{"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2}}"#;
    let stub = stub(&sse(&[AB, C, STOP, usage, DONE])).await;

    let events = events(model(&stub).stream(RawRequest::new("x"))).await;

    let expected = vec![
        StreamEvent::ContentDelta("ab".to_string()),
        StreamEvent::ContentDelta("c".to_string()),
        StreamEvent::Usage(Usage {
            input_tokens: 5,
            output_tokens: 2,
            ..Usage::default()
        }),
        StreamEvent::Stop(StopReason::EndTurn),
        StreamEvent::MessageStop,
    ];
    assert_eq!(events, expected);
}

/// F-1: a chunk that is not JSON is `Error::Json`.
#[tokio::test]
async fn malformed_chunk_is_json_error() {
    let stub = stub(&sse(&["not-json"])).await;

    let error = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect_err("malformed");

    assert!(matches!(error, Error::Json(_)), "got {error:?}");
}

/// F-2: an error chunk is `Error::Api` with the server's message.
#[tokio::test]
async fn error_chunk_is_api_error() {
    let chunk = r#"{"error":{"message":"slot unavailable","type":"server_error"}}"#;
    let stub = stub(&sse(&[chunk])).await;

    let error = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect_err("error chunk");

    let Error::Api(api) = error else {
        panic!("expected Api, got {error:?}");
    };
    assert_eq!(api.message(), "slot unavailable");
}

/// F-3: a body that ends after `"ab"`, without a finish or `[DONE]`, is
/// `Error::UnexpectedEnd`.
#[tokio::test]
async fn truncated_stream_is_unexpected_end() {
    let stub = stub(&sse(&[AB])).await;

    let error = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect_err("truncated");

    assert!(matches!(error, Error::UnexpectedEnd), "got {error:?}");
}

/// F-4: a 200 event stream with an empty body is `Error::EmptyResponse`.
#[tokio::test]
async fn empty_body_is_empty_response() {
    let stub = stub("").await;

    let error = model(&stub)
        .complete(RawRequest::new("x"))
        .await
        .expect_err("empty");

    assert!(matches!(error, Error::EmptyResponse), "got {error:?}");
}

/// S-1: temperature 0.2, top-p 0.9, seed 7 and stop `</s>` reach the body.
#[tokio::test]
async fn sampling_reaches_the_body() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let sampling = Sampling::new()
        .temperature(0.2)
        .top_p(0.9)
        .seed(7)
        .stop(["</s>"]);

    model(&stub)
        .complete(RawRequest::new("x").sampling(sampling))
        .await
        .expect("complete");

    let expected = json!({
        "model": "qwen3-8b",
        "prompt": "x",
        "max_tokens": 4096,
        "stream": true,
        "stream_options": {"include_usage": true},
        "temperature": 0.2_f64,
        "top_p": 0.9_f64,
        "seed": 7,
        "stop": ["</s>"]
    });
    assert_eq!(sent_body(&stub), expected);
}

/// S-3: a NaN temperature is `NonFinite("temperature")`; nothing is sent.
#[tokio::test]
async fn nan_temperature_fails_before_io() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let request = RawRequest::new("x").sampling(Sampling::new().temperature(f64::NAN));

    let error = model(&stub).complete(request).await.expect_err("nan");

    assert_eq!(request_error(error), RequestError::NonFinite("temperature"));
    assert_eq!(stub.requests().len(), 0);
}

/// `extra` with the one key `key` set to `value`.
fn extra(key: &str, value: Value) -> Map<String, Value> {
    let mut extra = Map::new();
    extra.insert(key.to_string(), value);
    extra
}

/// X-1: an extra key `top_k: 20` goes to the top level of the body.
#[tokio::test]
async fn extra_key_reaches_top_level() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let request = RawRequest::new("x").extra(extra("top_k", json!(20)));

    model(&stub).complete(request).await.expect("complete");

    let expected = json!({
        "model": "qwen3-8b",
        "prompt": "x",
        "max_tokens": 4096,
        "stream": true,
        "stream_options": {"include_usage": true},
        "top_k": 20
    });
    assert_eq!(sent_body(&stub), expected);
}

/// X-2: an extra key `prompt` is `ReservedKey("prompt")`; nothing is sent.
#[tokio::test]
async fn reserved_extra_key_fails_before_io() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let request = RawRequest::new("x").extra(extra("prompt", json!("y")));

    let error = model(&stub).complete(request).await.expect_err("reserved");

    assert_eq!(
        request_error(error),
        RequestError::ReservedKey("prompt".to_string())
    );
    assert_eq!(stub.requests().len(), 0);
}

/// M-1: a raw request on a Bedrock channel answers
/// `Unsupported("Bedrock has no raw completion")`; the stub sees no request.
#[tokio::test]
async fn bedrock_raw_is_unsupported_before_io() {
    let stub = StubServer::start(vec![]).await;
    let model = Bedrock::builder("ABSK-k", "eu-central-1")
        .expect("a valid region")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .openai()
        .raw("m");

    let result = model.complete(RawRequest::new("x")).await;

    match result {
        Err(Error::Unsupported(what)) => assert_eq!(what, "Bedrock has no raw completion"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
    assert_eq!(stub.requests().len(), 0);
}

/// P2: sampling and a stop sequence on a `RawRequest` reach the body as
/// `temperature` 0.2 and `stop` `["<|im_end|>"]`, with prompt and model.
#[tokio::test]
async fn p2_sampling_reaches_the_body() {
    let stub = stub(&sse(&[AB, C, STOP, DONE])).await;
    let sampling = Sampling::new().temperature(0.2).stop(["<|im_end|>"]);

    model(&stub)
        .complete(RawRequest::new("p").sampling(sampling))
        .await
        .expect("complete");

    let sent = sent_body(&stub);
    assert_eq!(sent["prompt"], "p");
    assert_eq!(sent["model"], "qwen3-8b");
    assert_eq!(sent["temperature"], json!(0.2));
    assert_eq!(sent["stop"], json!(["<|im_end|>"]));
}

/// A raw request is retried like any other: a 529 before any event, then
/// "ok" on the second call (restored from the 7f1e9a6 retry tests; the
/// scripted `Protocol<RawRequest>` is gone, so a stub plays it).
#[tokio::test]
async fn raw_request_is_retried() {
    let busy = StubReply::full(529, "application/json", r#"{"error":{"message":"busy"}}"#);
    let ok = StubReply::full(200, "text/event-stream", &sse(&[AB, C, STOP, DONE]));
    let stub = StubServer::start_replies(vec![(("POST", "/v1/completions"), vec![busy, ok])]).await;
    let model = OpenAi::builder(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .sleeper(std::sync::Arc::new(
            crate::provider::test_stub::RecordingSleeper::default(),
        ))
        .build()
        .raw("m");

    let completion = model
        .complete(RawRequest::new("p"))
        .await
        .expect("complete");

    assert_eq!(completion.text, "abc");
    assert_eq!(stub.requests().len(), 2);
}
