//! Tests for Claude on Amazon Bedrock: the origin and route, the credential
//! headers, region validation, streaming and error mapping on that route.

use tokio_stream::StreamExt;

use crate::provider::anthropic::client::Anthropic;
use crate::provider::anthropic::test_support::TEXT_SSE;
use crate::provider::auth::AuthMethod;
use crate::provider::error::{Error, ErrorKind};
use crate::provider::retry::RetryPolicy;
use crate::provider::test_stub::StubServer;

const MODEL: &str = "eu.anthropic.claude-haiku-5-5";
const BEDROCK_MESSAGES: &str = "/anthropic/v1/messages";

/// A stub answering the Bedrock Messages route with `status` and `body`.
async fn bedrock_stub(status: u16, content_type: &'static str, body: String) -> StubServer {
    StubServer::start(vec![(
        ("POST", BEDROCK_MESSAGES),
        (status, content_type, body),
    )])
    .await
}

/// A Bedrock client for `credential` whose origin is the stub at `url`.
fn bedrock_at(credential: impl Into<AuthMethod>, url: &str) -> Anthropic {
    Anthropic::builder(credential, MODEL)
        .bedrock("eu-central-1")
        .base_url(url)
        .retry(RetryPolicy::none())
        .build()
}

/// `ask` posts to `/anthropic/v1/messages` with the key as `x-api-key`, no
/// beta header, and the Bedrock model id in the body.
#[tokio::test]
async fn bedrock_ask_posts_key_and_model_under_route() {
    let stub = bedrock_stub(200, "text/event-stream", TEXT_SSE.to_string()).await;
    let claude = bedrock_at("ABSK-test", &stub.url());

    assert_eq!(claude.ask("x").await.expect("ask"), "Hi");
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, BEDROCK_MESSAGES);
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("ABSK-test")
    );
    assert_eq!(requests[0].headers.get("anthropic-beta"), None);
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).expect("json body");
    assert_eq!(body["model"], MODEL);
}

/// A Bedrock client with a bearer credential sends it as `Authorization`
/// without the OAuth beta header.
#[tokio::test]
async fn bedrock_bearer_sends_no_oauth_beta() {
    let stub = bedrock_stub(200, "text/event-stream", TEXT_SSE.to_string()).await;
    let claude = bedrock_at(AuthMethod::Bearer("ABSK-bearer".into()), &stub.url());

    claude.ask("x").await.expect("ask");
    let requests = stub.requests();
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer ABSK-bearer")
    );
    assert_eq!(requests[0].headers.get("anthropic-beta"), None);
}

/// `Anthropic::bedrock` targets the region's runtime route; `Debug` shows
/// it and hides the key.
#[test]
fn bedrock_origin_is_region_runtime_route() {
    let claude = Anthropic::bedrock("ABSK-secret-key", "eu-central-1", MODEL);

    let debug = format!("{claude:?}");
    assert!(
        debug.contains("\"https://bedrock-runtime.eu-central-1.amazonaws.com/anthropic\""),
        "{debug}"
    );
    assert!(!debug.contains("ABSK-secret-key"), "{debug}");
}

/// A region not of the form `eu-central-1` fails the first request with an
/// error naming it, before any request is sent.
#[tokio::test]
async fn bedrock_invalid_region_fails_before_io() {
    let stub = bedrock_stub(200, "text/event-stream", TEXT_SSE.to_string()).await;
    let claude = Anthropic::builder("ABSK-test", MODEL)
        .bedrock("Frankfurt")
        .base_url(stub.url())
        .build();

    let error = claude.ask("x").await.expect_err("invalid region");
    assert!(
        matches!(&error, Error::InvalidRegion(region) if region == "Frankfurt"),
        "{error:?}"
    );
    assert!(error.to_string().contains("Frankfurt"), "{error}");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(stub.requests().len(), 0);
}

/// Streaming on the Bedrock route yields the text deltas unchanged.
#[tokio::test]
async fn bedrock_stream_yields_deltas() {
    let sse = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n\
               event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n\
               event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n\
               event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n\
               event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let stub = bedrock_stub(200, "text/event-stream", sse.to_string()).await;
    let claude = bedrock_at("ABSK-test", &stub.url());

    let deltas: Vec<String> = claude
        .request("x")
        .stream()
        .collect::<Result<_, _>>()
        .await
        .expect("stream");
    assert_eq!(deltas, ["Hel", "lo"]);
}

/// A 403 `permission_error` from Bedrock maps to the crate's kind for it
/// and keeps the message.
#[tokio::test]
async fn bedrock_permission_error_maps_to_auth() {
    let body = r#"{"type":"error","error":{"type":"permission_error","message":"Invalid API Key format: Delimiter ':' not found"}}"#;
    let stub = bedrock_stub(403, "application/json", body.to_string()).await;
    let claude = bedrock_at("ABSK-test", &stub.url());

    let error = claude.ask("x").await.expect_err("403");
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(
        error
            .to_string()
            .contains("Invalid API Key format: Delimiter ':' not found"),
        "{error}"
    );
    assert_eq!(stub.requests().len(), 1);
}
