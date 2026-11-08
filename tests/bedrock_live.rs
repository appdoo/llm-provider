//! Live tests against Claude on Amazon Bedrock in `eu-central-1`. Ignored by
//! default; run with `AWS_BEARER_TOKEN_BEDROCK` set and `--ignored`. The key
//! is read here, never by the crate.
#![cfg(feature = "schema")]

use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;
use tokio_stream::StreamExt;

const REGION: &str = "eu-central-1";
const HAIKU: &str = "eu.anthropic.claude-haiku-5-5";
const OPUS: &str = "eu.anthropic.claude-opus-5-5";

/// What a text is about.
#[derive(Debug, Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

/// A Bedrock client for `model` with the key from the environment.
fn bedrock(model: &str) -> Anthropic {
    let key = std::env::var("AWS_BEARER_TOKEN_BEDROCK").expect("AWS_BEARER_TOKEN_BEDROCK set");
    Anthropic::bedrock(key, REGION, model)
}

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_ask_answers() {
    let answer = bedrock(HAIKU)
        .ask("What is a borrow checker? One sentence.")
        .await
        .expect("ask");

    println!("{answer}");
    assert!(!answer.trim().is_empty());
}

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_typed_answer_parses() {
    let summary: Summary = bedrock(OPUS)
        .request("Summarize: Rust's borrow checker enforces ownership and borrowing rules at compile time, preventing data races and use-after-free bugs without a garbage collector.")
        .send_json()
        .await
        .expect("typed answer");

    println!("{summary:?}");
    assert!(!summary.sentences.is_empty());
    assert!(!summary.key_points.is_empty());
}

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_stream_yields_several_deltas() {
    let deltas: Vec<String> = bedrock(HAIKU)
        .request("Count from 1 to 20, separated by commas.")
        .stream()
        .collect::<Result<_, _>>()
        .await
        .expect("stream");

    println!("{} deltas", deltas.len());
    assert!(deltas.len() >= 2);
}
