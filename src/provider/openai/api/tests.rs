//! Tests for one attempt against `/chat/completions`: the body sent, the
//! credential header, error answers and the provider info.

use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::provider::error::ErrorKind;
use crate::provider::message::Message;
use crate::provider::request::ModelSelection;
use crate::provider::test_stub::{RecordedRequest, StubServer};
use crate::provider::StopReason;

/// A recorded stream answering "Hello", usage 9/2.
const TEXT_SSE: &str = include_str!("../../../../tests/fixtures/openai_text.sse");

/// The header carrying the credential.
const AUTHORIZATION: &str = "authorization";

/// A stub answering `/chat/completions` with `status`, `content_type` and
/// `body`.
async fn stub(status: u16, content_type: &'static str, body: &str) -> StubServer {
    let route = ("POST", "/chat/completions");
    StubServer::start(vec![(route, (status, content_type, body.to_string()))]).await
}

/// A request saying `text` with the default model.
fn say(text: &str) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message::user(text)]),
        tools: vec![],
        max_tokens: 64,
        cache_breakpoints: Vec::new(),
    }
}

/// The body of `request` as JSON.
fn body_of(request: &RecordedRequest) -> Value {
    serde_json::from_str(&request.body).expect("json body")
}

/// Text folds to "Hello"; without a credential no `Authorization` header
/// is sent; the body streams and asks for usage.
#[tokio::test]
async fn complete_against_stub_without_key() {
    let stub = stub(200, "text/event-stream", TEXT_SSE).await;
    let api = OpenAiBuilder::new(&stub.url(), "llama3.2".into()).api();

    let completion = api.complete(say("Hi")).await.expect("complete");

    assert_eq!(completion.text, "Hello");
    assert_eq!(completion.stop, StopReason::EndTurn);
    assert_eq!(
        (
            completion.usage.input_tokens,
            completion.usage.output_tokens
        ),
        (9, 2)
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].headers.contains_key(AUTHORIZATION));
    let expected = json!({
        "model": "llama3.2",
        "messages": [{"role": "user", "content": "Hi"}],
        "max_tokens": 64,
        "stream": true,
        "stream_options": {"include_usage": true}
    });
    assert_eq!(body_of(&requests[0]), expected);
}

/// With a credential, `Authorization: Bearer <key>` is sent; a trailing `/`
/// on `base_url` is trimmed.
#[tokio::test]
async fn bearer_header_with_key_and_trailing_slash() {
    let stub = stub(200, "text/event-stream", TEXT_SSE).await;
    let api = OpenAiBuilder::new(&format!("{}/", stub.url()), "gpt-4o-mini".into())
        .credential("sk-abc")
        .api();

    api.complete(say("Hi")).await.expect("complete");

    let requests = stub.requests();
    assert_eq!(requests[0].path, "/chat/completions");
    assert_eq!(
        requests[0].headers.get(AUTHORIZATION).map(String::as_str),
        Some("Bearer sk-abc")
    );
}

/// A non-2xx answer is one `Api` error carrying status and body.
#[tokio::test]
async fn non_2xx_is_api_error_with_body() {
    let stub = stub(401, "application/json", r#"{"error":"bad key"}"#).await;
    let api = OpenAiBuilder::new(&stub.url(), "m".into()).api();

    match api.complete(say("Hi")).await {
        Err(Error::Api(e)) => {
            assert_eq!(e.status(), Some(401));
            assert_eq!(e.message(), r#"{"error":"bad key"}"#);
            assert_eq!(e.kind(), ErrorKind::Auth);
        }
        other => panic!("expected Api error, got {other:?}"),
    }
}

/// The info names the provider and its model.
#[test]
fn info_reports_openai_compatible() {
    let info = OpenAiBuilder::new("http://x/v1", "llama3.2".into())
        .api()
        .info();

    let expected = ProviderInfo {
        name: "openai-compatible".to_string(),
        model: "llama3.2".to_string(),
    };
    assert_eq!(info, expected);
}
