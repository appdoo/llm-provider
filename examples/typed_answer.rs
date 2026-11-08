//! A typed answer twice: `send_as` with a schema written by hand, and
//! `send_json` with the schema derived from the type.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example typed_answer --features schema
//! ```

use llm_provider::{Anthropic, JsonSchema, ResponseSchema};
use serde::Deserialize;
use serde_json::json;

const TEXT: &str = "Summarize: Rust is a systems programming language that guarantees \
memory safety without a garbage collector. Its borrow checker enforces ownership rules \
at compile time, and its type system makes data races a compile error.";

/// Sentences only, read against the hand-written schema.
#[derive(Debug, Deserialize)]
struct Sentences {
    sentences: Vec<String>,
}

/// What a text is about.
#[derive(Debug, Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key(), "claude-haiku-5-5");

    // A schema written by hand.
    let schema = ResponseSchema::new(
        "Sentences",
        json!({
            "type": "object",
            "properties": { "sentences": { "type": "array", "items": { "type": "string" } } },
            "required": ["sentences"]
        }),
    );
    let by_hand: Sentences = claude
        .request(TEXT)
        .system("Two to four sentences.")
        .send_as(schema)
        .await?;
    println!("send_as:   {}", by_hand.sentences.join(" "));

    // A schema derived from `Summary`; its doc comments are what the model reads.
    let derived = claude
        .request(TEXT)
        .system("Be brief.")
        .send_json::<Summary>()
        .await?;
    println!("send_json: {}", derived.sentences.join(" "));
    println!("key points: {}", derived.key_points.join("; "));
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!(
            "usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example typed_answer --features schema"
        );
        std::process::exit(2)
    })
}
