//! Tests for the provider factories and `ProviderConfig`.

use std::sync::Arc;

use super::*;
use crate::provider::test_stub::StubServer;

const TEXT_SSE: &str = include_str!("../../tests/fixtures/openai_text.sse");
const ANTHROPIC_SSE: &str = include_str!("../../tests/fixtures/anthropic_text.sse");

fn say(text: &str) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Default,
        system: None,
        messages: Arc::from(vec![Message {
            role: MessageRole::User,
            content: vec![Content::Text {
                text: text.to_string(),
            }],
        }]),
        tools: vec![],
        max_tokens: 64,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    }
}

async fn openai_stub() -> StubServer {
    StubServer::start(vec![(
        ("POST", "/chat/completions"),
        (200, "text/event-stream", TEXT_SSE.to_string()),
    )])
    .await
}

fn openai_config(base_url: String) -> ProviderConfig {
    ProviderConfig::default()
        .provider(ProviderKind::OpenAi)
        .model("m")
        .base_url(base_url)
}

/// An explicit credential builds the Anthropic provider.
#[tokio::test]
async fn create_provider_anthropic_with_explicit_credential() {
    let config: ProviderConfig =
        serde_json::from_str(r#"{"default":"anthropic"}"#).expect("anthropic is Claude");
    let provider = create_provider(&config, Some(AuthMethod::from("k")))
        .expect("explicit credential builds the provider");

    assert_eq!(provider.info().name, "anthropic");
}

/// The factory builds an OpenAI-compatible provider without a key; no
/// `authorization` header is sent.
#[tokio::test]
async fn create_provider_openai_without_key_sends_no_auth_header() {
    let stub = openai_stub().await;
    let provider =
        create_provider(&openai_config(stub.url()), None).expect("factory builds provider");

    provider.complete(say("Hi")).await.expect("complete");

    let requests = stub.requests();

    assert_eq!(requests.len(), 1);
    assert!(
        !requests[0].headers.contains_key("authorization"),
        "no auth header expected: {:?}",
        requests[0].headers
    );
}

/// The key passed to the factory reaches the request header.
#[tokio::test]
async fn create_provider_openai_key_travels_through_factory() {
    let stub = openai_stub().await;
    let provider = create_provider(&openai_config(stub.url()), Some(AuthMethod::from("sk-x")))
        .expect("factory builds provider");

    provider.complete(say("Hi")).await.expect("complete");

    let requests = stub.requests();

    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer sk-x")
    );
}

/// `create_provider` sends Anthropic traffic to `config.base_url`.
#[tokio::test]
async fn create_provider_anthropic_honours_base_url() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (200, "text/event-stream", ANTHROPIC_SSE.to_string()),
    )])
    .await;
    let config = ProviderConfig::default().base_url(stub.url());
    let provider = create_provider(&config, Some(AuthMethod::from("sk-test")))
        .expect("factory builds provider");

    provider.complete(say("Hi")).await.expect("complete");

    let requests = stub.requests();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/messages");
    assert_eq!(
        requests[0].headers.get("x-api-key").map(String::as_str),
        Some("sk-test")
    );
}

/// `create_anthropic` keeps `BatchProvider` and honours `base_url`.
#[tokio::test]
async fn create_anthropic_keeps_batches() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages/batches"),
        (
            200,
            "application/json",
            r#"{"id":"msgbatch_1","type":"message_batch","processing_status":"in_progress"}"#
                .to_string(),
        ),
    )])
    .await;
    let config = ProviderConfig::default().base_url(stub.url());
    let provider = create_anthropic(&config, AuthMethod::from("sk-test"));

    let request = BatchRequest::new("yt_abc", say("Hi")).expect("valid custom_id");
    let id = provider.submit(vec![request]).await.expect("submit");

    assert_eq!(id, BatchId::from("msgbatch_1".to_string()));
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/messages/batches");
}

/// The provider names a configuration file may use: `claude`, `anthropic`
/// and `openai`; any other name fails when the file is read.
#[test]
fn provider_kind_from_config_file() {
    let kind = |json: &str| serde_json::from_str::<ProviderConfig>(json).map(|c| c.provider);

    assert_eq!(
        kind(r#"{"default":"claude"}"#).expect("claude"),
        ProviderKind::Claude
    );
    assert_eq!(
        kind(r#"{"default":"anthropic"}"#).expect("anthropic"),
        ProviderKind::Claude
    );
    assert_eq!(
        kind(r#"{"default":"openai"}"#).expect("openai"),
        ProviderKind::OpenAi
    );
    assert_eq!(kind("{}").expect("default"), ProviderKind::Claude);
    assert!(kind(r#"{"default":"nope"}"#).is_err());
}

/// An OpenAI configuration without `base_url` or `model` is a
/// [`ConfigError`] naming what is missing.
#[test]
fn openai_config_needs_base_url_and_model() {
    let openai = ProviderConfig::default().provider(ProviderKind::OpenAi);

    let missing = |config: &ProviderConfig| match create_provider(config, None) {
        Err(Error::Config(error)) => error,
        Err(other) => panic!("expected Config, got {other:?}"),
        Ok(_) => panic!("expected Config, got a provider"),
    };

    assert_eq!(
        missing(&openai.clone().model("m")),
        ConfigError::MissingBaseUrl { provider: "openai" }
    );
    assert_eq!(
        missing(&openai.base_url("http://x")),
        ConfigError::MissingModel { provider: "openai" }
    );
}
