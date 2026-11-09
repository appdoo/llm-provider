//! Tests for the public client: `ask`, `complete`, the credential and its
//! redaction, the shared pool and the refusal of prompt caching.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::provider::completion::{Completion, ToolUseCall, Usage};
use crate::provider::error::Error;
use crate::provider::message::Message;
use crate::provider::request::{
    BreakpointPosition, CacheBreakpoint, CacheTtl, CompletionRequest, ModelSelection,
};
use crate::provider::test_stub::StubServer;
use crate::provider::test_support::Summary;
use crate::provider::{LlmProvider, StopReason};

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

    let local = OpenAi::at(stub.url()).llm("qwen3");
    let keyed = OpenAi::builder(stub.url())
        .credential("k")
        .build()
        .llm("qwen3");

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
    let client = OpenAi::at(stub.url()).llm("m");

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

    let completion = OpenAi::at(stub.url())
        .llm("m")
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

    let keyed = OpenAi::builder(stub.url())
        .credential("sk-1")
        .build()
        .llm("m");
    keyed.complete(hi()).await.expect("with credential");
    OpenAi::at(stub.url())
        .llm("m")
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

/// A-6: neither the client, its builder nor a role prints the credential;
/// the client and the role print exactly their address, protocol and model.
#[test]
fn debug_hides_credential() {
    let builder = OpenAi::builder("http://localhost:11434/v1").credential("sk-secret");
    let client = builder.clone().build();
    let llm = client.llm("m");

    let rendered = format!("{builder:?} {client:?} {llm:?}");

    assert!(!rendered.contains("sk-secret"), "got {rendered}");
    assert!(
        rendered.contains("credential: Some(ApiKey(\"***\"))"),
        "got {rendered}"
    );
    assert_eq!(
        format!("{client:?}"),
        r#"OpenAi { base_url: "http://localhost:11434/v1" }"#
    );
    assert_eq!(
        format!("{llm:?}"),
        r#"Llm { protocol: "openai", model: "m" }"#
    );
}

/// P-5: two roles from one client go out through the caller's one
/// `reqwest::Client`, one connection pool.
#[tokio::test]
async fn two_roles_share_one_client_and_one_pool() {
    let stub = stub(TEXT_SSE).await;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-pool", reqwest::header::HeaderValue::from_static("1"));
    let pooled = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("client");
    let client = OpenAi::builder(stub.url()).http_client(pooled).build();
    let a = client.llm("a");
    let b = client.llm("b");

    a.complete(hi()).await.expect("a");
    b.complete(hi()).await.expect("b");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.headers.get("x-pool").map(String::as_str), Some("1"));
    }
}

/// P-18: a request with a cache breakpoint is refused before anything is
/// sent, never silently dropped.
#[tokio::test]
async fn cache_breakpoint_is_refused_before_any_request() {
    let stub = stub(TEXT_SSE).await;
    let llm = OpenAi::at(stub.url()).llm("m");
    let mut request = hi();
    request.cache_breakpoints = vec![CacheBreakpoint {
        position: BreakpointPosition::System,
        ttl: CacheTtl::OneHour,
    }];

    let error = llm.complete(request).await.expect_err("refused");

    assert!(
        matches!(
            error,
            Error::Unsupported("prompt caching is not available with OpenAI")
        ),
        "got {error:?}"
    );
    assert_eq!(stub.requests().len(), 0);
}

/// J-2: the answer is the message text; the body asks for a `json_schema`
/// response format named after the schema, not in strict mode.
#[tokio::test]
async fn send_as_reads_the_text_and_sends_the_response_format() {
    let chunk = serde_json::json!({"choices":[{"index":0,
        "delta":{"role":"assistant","content":r#"{"sentences":["a"],"key_points":["b"]}"#},
        "finish_reason":"stop"}]});
    let sse = format!("data: {chunk}\n\ndata: [DONE]\n\n");
    let stub = stub(&sse).await;
    let openai = OpenAi::at(stub.url()).llm("qwen3");

    let summary: Summary = openai
        .request("x")
        .send_as(Summary::schema())
        .await
        .expect("send_as");

    assert_eq!(summary, Summary::ab());
    let body: serde_json::Value =
        serde_json::from_str(&stub.requests()[0].body).expect("json body");
    let format = &body["response_format"];
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["json_schema"]["name"], "Summary");
    assert_eq!(format["json_schema"]["strict"], false);
}

/// A 429 keeps the server's `error.type` and `error.message` in the
/// error's text.
#[tokio::test]
async fn rate_limit_error_names_the_server_message() {
    let body = r#"{"error":{"message":"Rate limit reached for requests","type":"requests"}}"#;
    let route = ("POST", "/chat/completions");
    let stub = StubServer::start(vec![(route, (429, "application/json", body.to_string()))]).await;
    let client = OpenAi::builder(stub.url())
        .retry(crate::provider::retry::RetryPolicy::none())
        .build()
        .llm("m");

    let text = client.ask("hi").await.expect_err("429").to_string();

    assert!(
        text.contains("API error (RateLimited, 429, requests): Rate limit reached for requests"),
        "{text}"
    );
}
