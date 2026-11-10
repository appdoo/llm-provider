//! Tests for the embedding model: two models on one endpoint, input order,
//! the credential, retry and the empty input.

use serde_json::json;

use super::*;
use crate::provider::test_stub::StubServer;
use crate::provider::{OpenAi, RetryPolicy};

/// A recorded stream answering "Hello".
const TEXT_SSE: &str = include_str!("../../../tests/fixtures/openai_text.sse");

/// An `/embeddings` answer listing input 1 before input 0.
const SWAPPED: &str = r#"{"data":[
    {"object":"embedding","index":1,"embedding":[1.0,2.0]},
    {"object":"embedding","index":0,"embedding":[0.5,0.25]}
]}"#;

/// The texts `["a", "b"]`.
fn a_b() -> Vec<String> {
    vec!["a".to_string(), "b".to_string()]
}

/// P-6: an embedder and an llm from one `OpenAi` both reach the stub under
/// `/v1`; the vectors come back in input order.
#[tokio::test]
async fn embedder_and_llm_share_one_endpoint() {
    let stub = StubServer::start(vec![
        (
            ("POST", "/v1/embeddings"),
            (200, "application/json", SWAPPED.to_string()),
        ),
        (
            ("POST", "/v1/chat/completions"),
            (200, "text/event-stream", TEXT_SSE.to_string()),
        ),
    ])
    .await;
    let openai = OpenAi::at(format!("{}/v1", stub.url()));

    let vectors = openai
        .embedder("bge-m3")
        .embed(&a_b())
        .await
        .expect("embed");
    let answer = openai.llm("q").ask("x").await.expect("ask");

    assert_eq!(vectors, vec![vec![0.5, 0.25], vec![1.0, 2.0]]);
    assert_eq!(answer, "Hello");
    let requests = stub.requests();
    let paths: Vec<&str> = requests.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["/v1/embeddings", "/v1/chat/completions"]);
    let body: serde_json::Value = serde_json::from_str(&requests[0].body).expect("json");
    assert_eq!(body, json!({"model": "bge-m3", "input": ["a", "b"]}));
}

/// P-6b: the embed request carries the credential; a 500 without retry is
/// `RetriesExhausted { attempts: 1, last: Api }`.
#[tokio::test]
async fn embed_failure_carries_credential_and_exhausts() {
    let stub = StubServer::start(vec![(
        ("POST", "/embeddings"),
        (500, "application/json", r#"{"error":"boom"}"#.to_string()),
    )])
    .await;
    let embedder = OpenAi::builder(stub.url())
        .credential("sk-x")
        .retry(RetryPolicy::none())
        .build()
        .embedder("m");

    let error = embedder.embed(&a_b()).await.expect_err("500");

    assert!(
        matches!(&error, Error::RetriesExhausted { attempts: 1, last } if matches!(**last, Error::Api(_))),
        "got {error:?}"
    );
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].headers.get("authorization").map(String::as_str),
        Some("Bearer sk-x")
    );
}

/// P-6b: `embed(&[])` is `Ok(vec![])` and sends no request.
#[tokio::test]
async fn empty_input_sends_nothing() {
    let stub = StubServer::start(vec![]).await;
    let embedder = OpenAi::at(stub.url()).embedder("m");

    let vectors = embedder.embed(&[]).await.expect("empty");

    assert_eq!(vectors, Vec::<Vec<f32>>::new());
    assert_eq!(stub.requests().len(), 0);
}

/// `Debug` names the model.
#[test]
fn debug_names_the_model() {
    let embedder = OpenAi::at("http://localhost:1/v1").embedder("bge-m3");

    assert_eq!(
        format!("{embedder:?}"),
        r#"Embedder { model: "bge-m3", .. }"#
    );
    assert_eq!(embedder.model(), "bge-m3");
}
