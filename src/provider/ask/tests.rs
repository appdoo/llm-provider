//! Tests for the one-question builder on `dyn LlmProvider`.

use crate::provider::test_stub::StubServer;
use crate::provider::{Anthropic, Error, LlmProvider};

async fn stub() -> StubServer {
    StubServer::start(vec![(
        ("POST", "/v1/messages"),
        (
            200,
            "text/event-stream",
            include_str!("../../../tests/fixtures/anthropic_text.sse").to_string(),
        ),
    )])
    .await
}

/// A8: `ask` works on a boxed trait object.
#[tokio::test]
async fn ask_on_boxed_dyn() {
    let stub = stub().await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .build();
    let p: Box<dyn LlmProvider> = Box::new(claude);
    assert_eq!(p.ask("Hello").await.expect("ask"), "Hi");
}

async fn via(p: &dyn LlmProvider) -> Result<String, Error> {
    p.ask("Hello").await
}

/// A8b: `ask` works on a borrowed `&dyn LlmProvider`.
#[tokio::test]
async fn ask_on_borrowed_dyn() {
    let stub = stub().await;
    let claude = Anthropic::builder("sk-test", "claude-x")
        .base_url(stub.url())
        .build();
    assert_eq!(via(&claude).await.expect("ask"), "Hi");
}
