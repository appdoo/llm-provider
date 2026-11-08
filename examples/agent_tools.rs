//! An agent with two `#[tool]` functions: one without input, one with a
//! typed input.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example agent_tools --features agent
//! ```

use std::convert::Infallible;
use std::time::{SystemTime, UNIX_EPOCH};

use llm_provider::agent::{tool, ConversationRuntime, Tools};
use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, std::time::SystemTimeError> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
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
async fn lines(range: LineRange) -> Result<String, Infallible> {
    let count = range.count.unwrap_or(usize::MAX);
    let lines = range.text.lines().skip(range.from).take(count);
    Ok(lines.collect::<Vec<_>>().join("\n"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key(), "claude-haiku-5-5");
    let tools = Tools::new().with(UnixTime).with(Lines);

    let mut agent = ConversationRuntime::new(claude, tools);
    let turn = agent
        .run_turn("What is the Unix time now? Then give me line 2 of \"a\\nb\\nc\".")
        .await?;
    println!("{}", turn.response);
    for call in &turn.tool_calls {
        println!("[{}] {} -> {}", call.name, call.input, call.output.content);
    }
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!(
            "usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example agent_tools --features agent"
        );
        std::process::exit(2)
    })
}
