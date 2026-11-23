//! Prompt caching on Claude: a long system prompt is written to the cache for
//! one hour on the first call and read back on the second, at a tenth of the
//! input price. `Usage` shows what happened.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example cache
//! AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example cache
//! ```
//!
//! With `ANTHROPIC_API_KEY` set it runs on the Anthropic API, otherwise on
//! Amazon Bedrock in `eu-central-1`.

use llm_provider::{Anthropic, Bedrock, CacheTtl, Llm};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sonnet = sonnet()?;

    // A prefix that stays the same, byte for byte, between calls: caching
    // starts above the model's minimum size (a few thousand tokens).
    let handbook = "Answer in one short sentence and stay polite. ".repeat(600);

    for question in ["What is a borrow checker?", "What is a lifetime?"] {
        let reply = sonnet
            .request(question)
            .system(handbook.as_str())
            .cache(CacheTtl::OneHour)
            .send()
            .await?;
        let usage = reply.usage;
        println!("{}", reply.text);
        println!(
            "  written {} (1h: {}), read {}, uncached {}\n",
            usage.cache_creation_input_tokens,
            usage.cache_creation_1h_input_tokens,
            usage.cache_read_input_tokens,
            usage.input_tokens,
        );
    }
    Ok(())
}

/// Claude Sonnet on the Anthropic API with `ANTHROPIC_API_KEY`, else on
/// Bedrock with `AWS_BEARER_TOKEN_BEDROCK`: the example reads the keys, the
/// crate never does. Exits 2 with a usage line when both are missing.
fn sonnet() -> Result<Llm, Box<dyn std::error::Error>> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        return Ok(Anthropic::new(key).llm(Anthropic::CLAUDE_SONNET_5_5));
    }

    let key = std::env::var("AWS_BEARER_TOKEN_BEDROCK").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… | AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example cache");
        std::process::exit(2)
    });
    let bedrock = Bedrock::new(key, "eu-central-1")?.anthropic();
    Ok(bedrock.llm(Bedrock::EU_CLAUDE_SONNET_5_5))
}
