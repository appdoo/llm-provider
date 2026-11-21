//! Tests for the one-question builder on `dyn LlmProvider`.

use std::time::Duration;

use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::provider::test_stub::{StubBody, StubReply, StubServer};
use crate::provider::{Anthropic, Ask, Error, Llm, LlmProvider, OpenAi};

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

/// `ask` works on a boxed trait object.
#[tokio::test]
async fn ask_on_boxed_dyn() {
    let stub = stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");
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
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");

    assert_eq!(via(&claude).await.expect("ask"), "Hi");
}

async fn via_generic<P: LlmProvider>(p: &P) -> Result<String, Error> {
    p.request("Hello").send().await.map(|c| c.text)
}

/// `request` works on a generic `P: LlmProvider`, with no cast.
#[tokio::test]
async fn request_on_generic_provider() {
    let stub = stub().await;
    let claude = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x");

    assert_eq!(via_generic(&claude).await.expect("request"), "Hi");
}

/// A stub streaming `texts` as chat chunks, then `[DONE]` when `done`, else
/// hanging.
async fn chat_stub(texts: &[&str], done: bool) -> StubServer {
    let mut lines: Vec<String> = texts
        .iter()
        .map(|text| {
            format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}\n\n")
        })
        .collect();
    if done {
        lines.push("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".into());
        lines.push("data: [DONE]\n\n".into());
    }
    let reply = StubReply {
        status: 200,
        content_type: "text/event-stream",
        headers: Vec::new(),
        body: StubBody::Events { lines },
    };
    StubServer::start_replies(vec![(("POST", "/v1/chat/completions"), vec![reply])]).await
}

/// The model `m` on the stub's `/v1` root.
fn chat_model(stub: &StubServer) -> Llm {
    OpenAi::at(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .llm("m")
}

/// X1: a token cancelled after "Hel" returns `Interrupted` with "Hel".
#[tokio::test]
async fn x1_cancel_returns_the_partial_text() {
    let stub = chat_stub(&["Hel"], false).await;
    let model = chat_model(&stub);
    let token = CancellationToken::new();

    let cancel = async {
        stub.written(1).await;
        sleep(Duration::from_millis(50)).await;
        token.cancel();
    };
    let (result, ()) = tokio::join!(model.request("hi").send_cancellable(&token), cancel);

    match result {
        Err(Error::Interrupted { partial_text, .. }) => assert_eq!(partial_text, "Hel"),
        other => panic!("expected Interrupted, got {other:?}"),
    }
}

/// X2: a token nobody cancels leaves the answer "Hello" whole.
#[tokio::test]
async fn x2_uncancelled_token_returns_the_answer() {
    let stub = chat_stub(&["Hel", "lo"], true).await;
    let model = chat_model(&stub);
    let token = CancellationToken::new();

    let completion = model
        .request("hi")
        .send_cancellable(&token)
        .await
        .expect("an answer");

    assert_eq!(completion.text, "Hello");
}
