//! Live tests against Claude on Amazon Bedrock in `eu-central-1`. Ignored by
//! default; run with `AWS_BEARER_TOKEN_BEDROCK` set and `--ignored`. The key
//! is read here, never by the crate.
#![cfg(feature = "schema")]

use llm_provider::{Bedrock, JsonSchema, Llm};
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

/// The Bedrock API key from the environment.
fn key() -> String {
    std::env::var("AWS_BEARER_TOKEN_BEDROCK").expect("AWS_BEARER_TOKEN_BEDROCK set")
}

/// Claude `model` on Bedrock with the key from the environment.
fn bedrock(model: &str) -> Llm {
    Bedrock::new(key(), REGION)
        .expect("a valid region")
        .anthropic()
        .llm(model)
}

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_ask_answers() {
    let answer = bedrock(HAIKU)
        .ask("What is a borrow checker? One sentence.")
        .await
        .expect("ask");

    println!("{answer}");
    assert_ne!(answer.trim(), "");
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
    assert_ne!(summary.sentences.len(), 0);
    assert_ne!(summary.key_points.len(), 0);
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

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_openai_route_answers_kimi() {
    let kimi = Bedrock::new(key(), REGION)
        .expect("a valid region")
        .openai()
        .llm(Bedrock::KIMI_K3);
    let done = kimi
        .request("What is a borrow checker? One sentence.")
        .max_tokens(1024)
        .send()
        .await
        .expect("send");

    println!("{}", done.text);
    assert_ne!(done.text.trim(), "");
}

#[tokio::test]
#[ignore = "live: Bedrock"]
async fn bedrock_one_hour_cache_is_written_then_read() {
    use llm_provider::CacheTtl;

    // Above every model's minimum cacheable size; the marker keeps an earlier
    // run's cache from turning the first call into a read.
    let marker = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let rules = "Rule: answer in one word. ".repeat(1200);
    let system = format!("Run {marker}. {rules}");
    let sonnet = bedrock("eu.anthropic.claude-sonnet-5-5");
    let ask = || {
        sonnet
            .request("Say ok.")
            .system(system.clone())
            .max_tokens(16)
            .cache(CacheTtl::OneHour)
            .send()
    };

    let first = ask().await.expect("first").usage;
    let second = ask().await.expect("second").usage;

    println!("first {first:?}\nsecond {second:?}");
    assert!(first.cache_creation_input_tokens > 0, "first call writes");
    assert!(first.cache_creation_1h_input_tokens > 0, "1h granted");
    assert!(second.cache_read_input_tokens > 0, "second call reads");
}
