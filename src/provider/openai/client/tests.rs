//! Tests for the public client: `ask`, `complete`, the credential and its
//! redaction.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::provider::completion::{Completion, ToolUseCall, Usage};
use crate::provider::message::Message;
use crate::provider::request::ModelSelection;
use crate::provider::test_stub::StubServer;
use crate::provider::StopReason;

/// A recorded stream answering "Hello".
const TEXT_SSE: &str = include_str!("../../../../tests/fixtures/openai_text.sse");

/// The header carrying the credential.
const AUTHORIZATION: &str = "authorization";

/// A stub answering every `/chat/completions` request with `body`.
async fn stub(body: &str) -> StubServer {
    let route = ("POST", "/chat/completions");
    StubServer::start(vec![(route, (200, "text/event-stream", body.to_string()))]).await
}

/// The model a request body names.
#[derive(serde::Deserialize)]
struct SentModel {
    model: String,
}

/// A request saying "hi" with the default model.
fn hi() -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message::user("hi")]),
        tools: vec![],
        max_tokens: 64,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    }
}

/// `ask` works without a credential and sends the model; a credential adds
/// the Bearer header.
#[tokio::test]
async fn front_ask_with_and_without_key() {
    let stub = stub(TEXT_SSE).await;

    let local = OpenAiCompatible::new(stub.url(), "qwen3");
    let keyed = OpenAiCompatible::builder(stub.url(), "qwen3")
        .credential("k")
        .build();

    assert_eq!(local.ask("Hello").await.expect("ask"), "Hello");
    assert_eq!(keyed.ask("Hello").await.expect("ask with key"), "Hello");
    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].headers.contains_key(AUTHORIZATION));
    assert_eq!(
        requests[1].headers.get(AUTHORIZATION).map(String::as_str),
        Some("Bearer k")
    );
    for request in &requests {
        let body: SentModel = serde_json::from_str(&request.body).expect("json");
        assert_eq!(body.model, "qwen3");
    }
}

/// A-1: chunks `"Hel"`, `"lo"`, `finish_reason: "stop"` and usage 3/2 ask
/// to `"Hello"` and complete to `EndTurn` with usage 3/2.
#[tokio::test]
async fn text_chunks_ask_and_complete() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
        "data: [DONE]\n\n"
    );
    let stub = stub(body).await;
    let client = OpenAiCompatible::new(stub.url(), "m");

    assert_eq!(client.ask("hi").await.expect("ask"), "Hello");
    let completion = client.complete(hi()).await.expect("complete");

    let expected = Completion {
        text: "Hello".to_string(),
        tool_uses: vec![],
        usage: Usage {
            input_tokens: 3,
            output_tokens: 2,
            ..Usage::default()
        },
        stop: StopReason::EndTurn,
    };
    assert_eq!(completion, expected);
}

/// A-3: a tool call in three fragments (id `c1`, name `read`, arguments
/// `{"pa` and `th":"a"}`) completes to one call with `stop == ToolUse`.
#[tokio::test]
async fn fragmented_tool_call_completes_to_one_call() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\"}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let stub = stub(body).await;

    let completion = OpenAiCompatible::new(stub.url(), "m")
        .complete(hi())
        .await
        .expect("complete");

    let call = ToolUseCall {
        id: "c1".to_string(),
        name: "read".to_string(),
        input: json!({"path": "a"}),
    };
    assert_eq!(completion.tool_uses, vec![call]);
    assert_eq!(completion.stop, StopReason::ToolUse);
}

/// A-5: `credential("sk-1")` sends `Authorization: Bearer sk-1`; without a
/// credential no `Authorization` header goes out.
#[tokio::test]
async fn credential_sets_bearer_header() {
    let stub = stub(TEXT_SSE).await;

    let keyed = OpenAiCompatible::builder(stub.url(), "m")
        .credential("sk-1")
        .build();
    keyed.complete(hi()).await.expect("with credential");
    OpenAiCompatible::new(stub.url(), "m")
        .complete(hi())
        .await
        .expect("without credential");

    let requests = stub.requests();
    assert_eq!(
        requests[0].headers.get(AUTHORIZATION).map(String::as_str),
        Some("Bearer sk-1")
    );
    assert!(!requests[1].headers.contains_key(AUTHORIZATION));
}

/// A-6: neither the client nor its builder prints the credential.
#[test]
fn debug_hides_credential() {
    let builder =
        OpenAiCompatible::builder("http://localhost:11434/v1", "m").credential("sk-secret");

    let rendered = format!("{builder:?} {:?}", builder.clone().build());

    assert!(!rendered.contains("sk-secret"), "got {rendered}");
    assert!(
        rendered.contains("credential: Some(ApiKey(\"***\"))"),
        "got {rendered}"
    );
}
