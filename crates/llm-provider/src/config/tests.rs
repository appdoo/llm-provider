//! Tests for `ModelBuilder`, `Protocol` and `ConfigError`: each row of the
//! build table against a recording stub, the refusals, the file keys and
//! the masked credential.

use serde_json::json;

use super::*;
use crate::provider::test_stub::{RecordedRequest, StubServer};
use crate::provider::{Anthropic, Bedrock, Embed, Embedder, Error, ErrorKind, Llm};

const OPENAI_SSE: &str = include_str!("../../tests/fixtures/openai_text.sse");
const ANTHROPIC_SSE: &str = include_str!("../../tests/fixtures/anthropic_text.sse");

/// The test key.
const K: &str = "sk-test";

/// A stub answering `path` with a recorded stream.
async fn stub(path: &'static str, sse: &str) -> StubServer {
    StubServer::start(vec![(
        ("POST", path),
        (200, "text/event-stream", sse.to_string()),
    )])
    .await
}

/// The only request the stub saw.
fn only_request(stub: &StubServer) -> RecordedRequest {
    let mut requests = stub.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    requests.remove(0)
}

/// A header of `request`, if sent.
fn header<'a>(request: &'a RecordedRequest, name: &str) -> Option<&'a str> {
    request.headers.get(name).map(String::as_str)
}

/// The `model` of a request body.
fn body_model(request: &RecordedRequest) -> String {
    let body: serde_json::Value = serde_json::from_str(&request.body).expect("json body");
    body["model"].as_str().expect("model").to_string()
}

/// The `ConfigError` inside `result`.
fn config_error<M: std::fmt::Debug>(result: Result<M, Error>) -> ConfigError {
    match result {
        Err(Error::Config(error)) => error,
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

/// The owner's line: an OpenAI-compatible server with a key.
#[tokio::test]
async fn openai_model_by_hand() {
    let stub = stub("/v1/chat/completions", OPENAI_SSE).await;
    let qwen = Llm::builder("qwen3")
        .protocol(Protocol::OpenAi)
        .endpoint(format!("{}/v1", stub.url()))
        .api_key(K)
        .build()
        .expect("build");

    qwen.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(header(&request, "authorization"), Some("Bearer sk-test"));
    assert_eq!(body_model(&request), "qwen3");
}

/// An API key on the Anthropic API goes as `x-api-key`.
#[tokio::test]
async fn anthropic_model_with_api_key() {
    let stub = stub("/v1/messages", ANTHROPIC_SSE).await;
    let haiku = Llm::builder(Anthropic::CLAUDE_HAIKU_5_5)
        .endpoint(stub.url())
        .api_key(K)
        .build()
        .expect("build");

    haiku.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(header(&request, "x-api-key"), Some("sk-test"));
    assert_eq!(header(&request, "authorization"), None);
    assert_eq!(body_model(&request), "claude-haiku-5-5");
}

/// An OAuth token goes as a bearer token with the OAuth beta header.
#[tokio::test]
async fn anthropic_model_with_oauth_token() {
    let stub = stub("/v1/messages", ANTHROPIC_SSE).await;
    let haiku = Llm::builder(Anthropic::CLAUDE_HAIKU_5_5)
        .endpoint(stub.url())
        .api_key(K)
        .oauth_token("sk-ant-oat01-x")
        .build()
        .expect("build");

    haiku.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(
        header(&request, "authorization"),
        Some("Bearer sk-ant-oat01-x")
    );
    let beta = header(&request, "anthropic-beta").expect("anthropic-beta");
    assert!(beta.contains("oauth-2025-04-20"), "{beta}");
    assert_eq!(header(&request, "x-api-key"), None);
}

/// Bedrock with the OpenAI protocol posts under `/openai/v1`.
#[tokio::test]
async fn bedrock_openai_model() {
    let stub = stub("/openai/v1/chat/completions", OPENAI_SSE).await;
    let kimi = Llm::builder(Bedrock::KIMI_K3)
        .protocol(Protocol::OpenAi)
        .bedrock("eu-central-1")
        .endpoint(stub.url())
        .api_key("ABSK-x")
        .build()
        .expect("build");

    kimi.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(request.path, "/openai/v1/chat/completions");
    assert_eq!(header(&request, "authorization"), Some("Bearer ABSK-x"));
}

/// A configuration file names protocol, region, model and endpoint; the
/// credential is set in code.
#[tokio::test]
async fn model_from_file() {
    let stub = stub("/anthropic/v1/messages", ANTHROPIC_SSE).await;
    let file = json!({
        "protocol": "anthropic",
        "region": "eu-central-1",
        "model": "eu.anthropic.claude-opus-5-5",
        "base_url": stub.url(),
    });
    let opus = serde_json::from_value::<ModelBuilder<Llm>>(file)
        .expect("file reads")
        .api_key("ABSK-x")
        .build()
        .expect("build");

    opus.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(request.path, "/anthropic/v1/messages");
    assert_eq!(header(&request, "x-api-key"), Some("ABSK-x"));
    assert_eq!(body_model(&request), "eu.anthropic.claude-opus-5-5");
}

/// A file with the earlier keys `default` and `base_url` still reads, and
/// Anthropic without `model` is Claude Sonnet 5.5.
#[tokio::test]
async fn earlier_file_keys_still_read() {
    let stub = stub("/v1/messages", ANTHROPIC_SSE).await;
    let file = json!({"default": "claude", "base_url": stub.url()});
    let sonnet = serde_json::from_value::<ModelBuilder<Llm>>(file)
        .expect("file reads")
        .api_key(K)
        .build()
        .expect("build");

    sonnet.ask("x").await.expect("ask");

    let request = only_request(&stub);
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(body_model(&request), "claude-sonnet-5-5");
}

/// The Anthropic API without a credential is refused as an `Auth` error.
#[test]
fn anthropic_without_credential_is_refused() {
    let error = config_error(Llm::builder("m").build());

    assert_eq!(
        error,
        ConfigError::MissingCredential {
            provider: "anthropic"
        }
    );
    assert_eq!(error.to_string(), "anthropic provider needs a credential");
    assert_eq!(Error::Config(error).kind(), ErrorKind::Auth);
}

/// An OpenAI-compatible server without an endpoint is refused.
#[test]
fn openai_without_endpoint_is_refused() {
    let error = config_error(Llm::builder("m").protocol(Protocol::OpenAi).build());

    assert_eq!(error, ConfigError::MissingEndpoint { provider: "openai" });
    assert_eq!(Error::Config(error).kind(), ErrorKind::InvalidRequest);
}

/// An OpenAI-compatible server without a model is refused.
#[test]
fn openai_without_model_is_refused() {
    let file = json!({"protocol": "openai", "endpoint": "http://x"});
    let builder = serde_json::from_value::<ModelBuilder<Llm>>(file).expect("file reads");

    let error = config_error(builder.build());

    assert_eq!(error, ConfigError::MissingModel { provider: "openai" });
}

/// Bedrock without a credential is refused.
#[test]
fn bedrock_without_credential_is_refused() {
    let builder = Llm::builder(Bedrock::KIMI_K3)
        .protocol(Protocol::OpenAi)
        .bedrock("eu-central-1");

    let error = config_error(builder.build());

    assert_eq!(
        error,
        ConfigError::MissingCredential {
            provider: "bedrock"
        }
    );
}

/// An embedding model on the Anthropic protocol is unsupported.
#[test]
fn anthropic_embedder_is_refused() {
    let result = Embedder::builder("bge-m3").api_key(K).build();

    match result {
        Err(Error::Unsupported(text)) => {
            assert_eq!(text, "Anthropic has no embedding models");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

/// An embedding model on an OpenAI-compatible server posts to
/// `/embeddings`.
#[tokio::test]
async fn openai_embedder_by_hand() {
    let answer = r#"{"data":[{"object":"embedding","index":0,"embedding":[0.5]}]}"#;
    let stub = StubServer::start(vec![(
        ("POST", "/v1/embeddings"),
        (200, "application/json", answer.to_string()),
    )])
    .await;
    let bge = Embedder::builder("bge-m3")
        .protocol(Protocol::OpenAi)
        .endpoint(format!("{}/v1", stub.url()))
        .build()
        .expect("build");

    let vectors = bge.embed(&["a".to_string()]).await.expect("embed");

    assert_eq!(vectors, vec![vec![0.5]]);
    let request = only_request(&stub);
    assert_eq!(request.path, "/v1/embeddings");
    let body: serde_json::Value = serde_json::from_str(&request.body).expect("json");
    assert_eq!(body, json!({"model": "bge-m3", "input": ["a"]}));
}

/// Protocol names parse in any case; `claude` is Anthropic; an unknown
/// name is an error naming both protocols.
#[test]
fn protocol_names_parse() {
    assert_eq!("openai".parse::<Protocol>(), Ok(Protocol::OpenAi));
    assert_eq!("claude".parse::<Protocol>(), Ok(Protocol::Anthropic));
    assert_eq!("Anthropic".parse::<Protocol>(), Ok(Protocol::Anthropic));

    let error = "gemini".parse::<Protocol>().expect_err("unknown");

    assert_eq!(
        error,
        ConfigError::UnknownProtocol {
            name: "gemini".to_string()
        }
    );
    let message = error.to_string();
    assert!(
        message.contains("anthropic") && message.contains("openai"),
        "{message}"
    );
}

/// The credential shows its kind in `Debug` and never reaches the JSON.
#[test]
fn credential_is_masked() {
    let builder = Llm::builder("m").api_key("sk-secret");

    let debug = format!("{builder:?}");
    let written = serde_json::to_string(&builder).expect("serialises");

    assert!(debug.contains("ApiKey"), "{debug}");
    assert!(!debug.contains("sk-secret"), "{debug}");
    assert!(!written.contains("sk-secret"), "{written}");
    assert!(!written.contains("credential"), "{written}");
}

/// `max_attempts` reaches the endpoint: one attempt against a 529.
#[tokio::test]
async fn retry_setting_reaches_the_endpoint() {
    let stub = StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (529, "application/json", OVERLOADED.to_string()),
    )])
    .await;
    let model = Llm::builder("m")
        .endpoint(stub.url())
        .max_attempts(1)
        .api_key(K)
        .build()
        .expect("build");

    let error = model.ask("x").await.expect_err("overloaded");

    assert!(
        matches!(error, Error::RetriesExhausted { attempts: 1, .. }),
        "{error:?}"
    );
    assert_eq!(stub.requests().len(), 1);
}

/// An Anthropic overloaded answer.
const OVERLOADED: &str = r#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
