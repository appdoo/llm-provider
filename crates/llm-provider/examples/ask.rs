//! One question to Claude, then the same question with a system prompt and
//! a token limit.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example ask
//! ```

use llm_provider::Anthropic;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key()).llm(Anthropic::CLAUDE_HAIKU_5_5);

    let answer = claude.ask("What is a borrow checker?").await?;
    println!("{answer}\n");

    let reply = claude
        .request("What is a borrow checker?")
        .system("Answer in one sentence.")
        .max_tokens(256)
        .send()
        .await?;
    println!("{}", reply.text);
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example ask");
        std::process::exit(2)
    })
}
