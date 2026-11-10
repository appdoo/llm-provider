//! Tests for the shared endpoint builder: one trim of the origin (R-2) and a
//! recording sleeper on an endpoint other than Anthropic (R-3).

use std::sync::Arc;

use crate::provider::test_stub::{RecordingSleeper, StubReply, StubServer};
use crate::provider::{Anthropic, Bedrock, OpenAi, RetryPolicy};

const ANTHROPIC_SSE: &str = include_str!("../../../tests/fixtures/anthropic_text.sse");
const OPENAI_SSE: &str = include_str!("../../../tests/fixtures/openai_text.sse");

/// A route answering every request with `body` as an event stream.
fn sse(path: &'static str, body: &str) -> ((&'static str, &'static str), Vec<StubReply>) {
    (
        ("POST", path),
        vec![StubReply::full(200, "text/event-stream", body)],
    )
}

/// R-2: a trailing `/` on the address of each endpoint never doubles a
/// slash in the request path.
#[tokio::test]
async fn trailing_slash_never_doubles_a_slash() {
    let stub = StubServer::start_replies(vec![
        sse("/v1/messages", ANTHROPIC_SSE),
        sse("/v1/chat/completions", OPENAI_SSE),
        sse("/anthropic/v1/messages", ANTHROPIC_SSE),
    ])
    .await;
    let origin = format!("{}/", stub.url());

    let anthropic = Anthropic::builder("k").base_url(&origin).build();
    let openai = OpenAi::builder(format!("{origin}v1/")).build();
    let bedrock = Bedrock::builder("k", "eu-central-1")
        .base_url(&origin)
        .build();
    anthropic.llm("m").ask("hi").await.expect("anthropic");
    openai.llm("m").ask("hi").await.expect("openai");
    bedrock
        .anthropic()
        .llm("m")
        .ask("hi")
        .await
        .expect("bedrock");

    let paths: Vec<String> = stub.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(
        paths,
        [
            "/v1/messages",
            "/v1/chat/completions",
            "/anthropic/v1/messages"
        ]
    );
    assert!(paths.iter().all(|path| !path.contains("//")), "{paths:?}");
}

/// R-3: an `OpenAi` endpoint waits through the sleeper its builder was
/// given: a 529 then a 200 is one recorded wait and an answer.
#[tokio::test]
async fn openai_retries_through_the_builders_sleeper() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/chat/completions"),
        vec![
            StubReply::full(529, "application/json", "{}"),
            StubReply::full(200, "text/event-stream", OPENAI_SSE),
        ],
    )])
    .await;
    let recorder = Arc::new(RecordingSleeper::default());

    let openai = OpenAi::builder(stub.url())
        .retry(RetryPolicy::default())
        .sleeper(recorder.clone())
        .build();
    let answer = openai.llm("m").ask("hi").await;

    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(recorder.slept().len(), 1);
}
