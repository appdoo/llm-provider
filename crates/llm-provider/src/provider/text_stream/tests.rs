//! Tests for [`TextStream`](super::TextStream) and the streaming methods of
//! the request builder, over a stub server speaking real SSE, so the
//! providers' own decoders are on the path.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio_stream::StreamExt;

use super::TextStream;
use crate::provider::test_stub::{RecordingSleeper, StubReply, StubServer};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{
    Anthropic, ApiError, Ask, Error, ErrorKind, Llm, OpenAi, StopReason, StreamEvent,
    ToolDefinition, ToolUseCall, Usage,
};

const MESSAGES: (&str, &str) = ("POST", "/v1/messages");
const CHAT: (&str, &str) = ("POST", "/chat/completions");
const OPENAI_TEXT_SSE: &str = include_str!("../../../tests/fixtures/openai_text.sse");
const OVERLOADED: &str =
    r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;

// `TextStream` is usable across tasks, needs no pinning and borrows nothing.
const _: fn() = || {
    fn assert_send_unpin_owned<T: Send + Unpin + 'static>() {}
    assert_send_unpin_owned::<TextStream>();
};

/// The events as a server-sent body, each under its `type`.
fn sse_body(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| {
            let name = event["type"].as_str().expect("event has a type");
            format!("event: {name}\ndata: {event}\n\n")
        })
        .collect::<Vec<_>>()
        .concat()
}

fn sse(body: &str) -> StubReply {
    StubReply::full(200, "text/event-stream", body)
}

/// `message_start` without the optional `usage`.
fn message_start() -> Value {
    json!({"type": "message_start", "message": {"id": "msg_1", "role": "assistant", "content": []}})
}

fn text_block_start() -> Value {
    json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})
}

fn text_delta(text: &str) -> Value {
    json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}})
}

fn block_stop(index: u32) -> Value {
    json!({"type": "content_block_stop", "index": index})
}

fn message_delta(stop_reason: &str, output_tokens: u32) -> Value {
    json!({"type": "message_delta",
           "delta": {"stop_reason": stop_reason, "stop_sequence": null},
           "usage": {"output_tokens": output_tokens}})
}

fn message_stop() -> Value {
    json!({"type": "message_stop"})
}

/// An Anthropic answer of `deltas`, using 3 input and 2 output tokens,
/// stopping at `end_turn`.
fn text_answer(deltas: &[&str]) -> String {
    let start = json!({"type": "message_start",
                       "message": {"id": "msg_1", "role": "assistant", "content": [],
                                   "usage": {"input_tokens": 3, "output_tokens": 0}}});

    let mut events = vec![start, text_block_start()];
    events.extend(deltas.iter().map(|text| text_delta(text)));
    events.extend([block_stop(0), message_delta("end_turn", 2), message_stop()]);

    sse_body(&events)
}

/// An Anthropic answer of the text `"x"`, then a call of the tool `read`,
/// with usage only on `message_delta`.
fn text_and_tool_answer() -> String {
    sse_body(&[
        message_start(),
        text_block_start(),
        text_delta("x"),
        block_stop(0),
        json!({"type": "content_block_start", "index": 1,
               "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {}}}),
        json!({"type": "content_block_delta", "index": 1,
               "delta": {"type": "input_json_delta", "partial_json": "{\"path\":\"a.txt\"}"}}),
        block_stop(1),
        message_delta("tool_use", 2),
        message_stop(),
    ])
}

async fn stub_answering(replies: Vec<StubReply>) -> StubServer {
    StubServer::start_replies(vec![(MESSAGES, replies)]).await
}

fn claude(stub: &StubServer) -> Llm {
    Anthropic::builder("test-key")
        .base_url(stub.url())
        .build()
        .llm("m")
}

/// The deltas of `stream`, which must end without an error.
async fn text_of(stream: TextStream) -> Vec<String> {
    stream
        .collect::<Result<Vec<_>, Error>>()
        .await
        .expect("stream without an error")
}

/// Asserts `results` is exactly the text `"a"`, then an `Overloaded` error.
fn assert_a_then_overloaded(results: &[Result<String, Error>]) {
    assert_eq!(results.len(), 2, "got {results:?}");
    assert_eq!(results[0].as_deref().expect("text first"), "a");

    let error = results[1].as_ref().expect_err("error second");
    assert_eq!(error.kind(), ErrorKind::Overloaded);
}

/// Text deltas come through in order; start, usage and stop events do not.
#[tokio::test]
async fn stream_yields_anthropic_text_deltas_in_order() {
    let stub = stub_answering(vec![sse(&text_answer(&["Hel", "lo"]))]).await;

    let deltas = text_of(claude(&stub).request("x").stream()).await;

    assert_eq!(deltas, ["Hel", "lo"]);
    assert_eq!(deltas.concat(), "Hello");
}

/// The same request on the OpenAI protocol gives the same deltas.
#[tokio::test]
async fn stream_yields_openai_text_deltas_in_order() {
    let stub = StubServer::start_replies(vec![(CHAT, vec![sse(OPENAI_TEXT_SSE)])]).await;
    let provider = OpenAi::at(stub.url()).llm("m");

    let deltas = text_of(provider.request("x").stream()).await;

    assert_eq!(deltas, ["Hel", "lo"]);
}

/// A server error event mid-answer is yielded once, after the text so far.
#[tokio::test]
async fn stream_yields_a_server_error_event_and_ends() {
    let body = format!(
        "{}event: error\ndata: {OVERLOADED}\n\n",
        sse_body(&[message_start(), text_block_start(), text_delta("a")])
    );
    let stub = stub_answering(vec![sse(&body)]).await;

    let results: Vec<_> = claude(&stub).request("x").stream().collect().await;

    assert_a_then_overloaded(&results);
}

/// `TextStream` itself ends at an error, even when the inner stream would
/// go on with more text.
#[tokio::test]
async fn stream_ends_at_an_error_though_the_inner_stream_goes_on() {
    let overloaded = ApiError::new(ErrorKind::Overloaded, "try later");
    let provider = ScriptedProvider::with_results([vec![
        Ok(StreamEvent::ContentDelta("a".to_string())),
        Err(Error::Api(overloaded)),
        Ok(StreamEvent::ContentDelta("b".to_string())),
    ]]);

    let results: Vec<_> = provider.request("x").stream().collect().await;

    assert_a_then_overloaded(&results);
}

/// A tool call in the answer is skipped; only its text comes through.
#[tokio::test]
async fn stream_skips_tool_use() {
    let stub = stub_answering(vec![sse(&text_and_tool_answer())]).await;

    let deltas = text_of(claude(&stub).request("x").stream()).await;

    assert_eq!(deltas, ["x"]);
}

/// `events` yields every event of the answer, in the provider's order.
#[tokio::test]
async fn events_yields_the_whole_answer_in_order() {
    let stub = stub_answering(vec![sse(&text_and_tool_answer())]).await;

    let events = claude(&stub)
        .request("x")
        .events()
        .collect::<Result<Vec<_>, Error>>()
        .await
        .expect("stream without an error");

    let tool_call = ToolUseCall {
        id: "toolu_1".to_string(),
        name: "read".to_string(),
        input: json!({"path": "a.txt"}),
    };
    let usage = Usage {
        output_tokens: 2,
        ..Usage::default()
    };
    assert_eq!(
        events,
        [
            StreamEvent::ContentDelta("x".to_string()),
            StreamEvent::ToolUse(tool_call),
            StreamEvent::Usage(usage),
            StreamEvent::Stop(StopReason::ToolUse),
            StreamEvent::MessageStop,
        ]
    );
}

/// A 529 before the first event is retried, unseen by the caller.
#[tokio::test]
async fn stream_retries_an_overloaded_server_before_the_first_event() {
    let overloaded = StubReply::full(529, "application/json", OVERLOADED);
    let stub = stub_answering(vec![overloaded, sse(&text_answer(&["ok"]))]).await;
    let provider = Anthropic::builder("test-key")
        .base_url(stub.url())
        .sleeper(Arc::new(RecordingSleeper::default()))
        .build()
        .llm("m");

    let deltas = text_of(provider.request("x").stream()).await;

    assert_eq!(deltas, ["ok"]);
    assert_eq!(stub.requests().len(), 2);
}

/// Tools set on the builder are in the body of the streamed request.
#[tokio::test]
async fn stream_sends_the_builders_tools() {
    let stub = stub_answering(vec![sse(&text_answer(&["ok"]))]).await;
    let tool = ToolDefinition {
        name: "read".to_string(),
        description: "Reads a file.".to_string(),
        input_schema: json!({"type": "object"}),
    };

    let deltas = text_of(claude(&stub).request("x").tools([tool.clone()]).stream()).await;

    assert_eq!(deltas, ["ok"]);
    let body: Value = serde_json::from_str(&stub.requests()[0].body).expect("JSON body");
    assert_eq!(body["stream"], true);
    assert_eq!(body["tools"][0]["name"], tool.name.as_str());
}
