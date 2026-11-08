//! M-8: the caller's program of Spec 37 against a stub Anthropic server: a
//! typed answer, then an agent turn that calls `unix_time` once.
#![cfg(feature = "agent")]

mod stub;

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use llm_provider::agent::{tool, ConversationRuntime, Tools};
use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;

use stub::Stub;

/// What a text is about.
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    #[allow(dead_code)] // The program prints only the sentences.
    key_points: Vec<String>,
}

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, std::io::Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(now.as_secs().to_string())
}

/// Which lines of a text to read.
#[derive(Deserialize, JsonSchema)]
struct LineRange {
    /// The text.
    text: String,
    /// The first line, counting from 0.
    from: usize,
    /// How many lines; all when left out.
    count: Option<usize>,
}

/// Returns some lines of a text.
#[tool]
async fn lines(range: LineRange) -> Result<String, std::convert::Infallible> {
    let lines = range
        .text
        .lines()
        .skip(range.from)
        .take(range.count.unwrap_or(usize::MAX));
    Ok(lines.collect::<Vec<_>>().join("\n"))
}

/// The caller's program, with the stub's URL in place of the key file and
/// its printing collected in `out`.
async fn program(claude: Anthropic, out: &mut String) -> Result<usize, Box<dyn std::error::Error>> {
    let summary: Summary = claude
        .ask_json("Summarize: Rust is a systems language …")
        .await?;
    writeln!(out, "{}", summary.sentences.join(" "))?;

    let mut agent = ConversationRuntime::new(claude, Tools::new().with(UnixTime).with(Lines));
    let turn = agent.run_turn("What is the Unix time now?").await?;
    writeln!(out, "{}", turn.response)?;
    Ok(turn.tool_calls)
}

/// An event stream whose answer is one `tool_use` call of `name` with
/// `input` (a JSON text).
fn tool_use_sse(id: &str, name: &str, input: &str) -> String {
    let start = serde_json::json!({"type":"content_block_start","index":0,
        "content_block":{"type":"tool_use","id":id,"name":name}});
    let delta = serde_json::json!({"type":"content_block_delta","index":0,
        "delta":{"type":"input_json_delta","partial_json":input}});
    format!(
        "event: content_block_start\ndata: {start}\n\n\
         event: content_block_delta\ndata: {delta}\n\n\
         event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n\
         event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"tool_use\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n\
         event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
    )
}

#[tokio::test]
async fn callers_program_prints_summary_and_turn() {
    let summary = r#"{"sentences":["Rust is fast."],"key_points":["speed"]}"#;
    let answer = include_str!("fixtures/anthropic_text.sse").to_string();
    let stub = Stub::start(vec![
        (
            "text/event-stream",
            tool_use_sse("toolu_1", "Summary", summary),
        ),
        (
            "text/event-stream",
            tool_use_sse("toolu_2", "unix_time", "{}"),
        ),
        ("text/event-stream", answer),
    ])
    .await;
    let claude = Anthropic::builder("sk-test", "claude-sonnet-4-6")
        .base_url(stub.url())
        .build();

    let mut out = String::new();
    let tool_calls = program(claude, &mut out).await.unwrap();

    assert_eq!(out, "Rust is fast.\nHi\n");
    assert_eq!(tool_calls, 1);
    let bodies = stub.bodies();
    let results = bodies[2].matches("\"tool_result\"").count();
    assert_eq!(results, 1, "{}", bodies[2]);
    assert!(bodies[2].contains("toolu_2"), "{}", bodies[2]);
}
