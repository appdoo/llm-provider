//! Claude and another model on Amazon Bedrock with one Bedrock API key:
//! Claude through Bedrock's Anthropic route, Kimi K3 through its
//! OpenAI-compatible route.
//!
//! ```sh
//! AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example bedrock
//! ```

use llm_provider::Bedrock;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bedrock = Bedrock::new(bedrock_key(), "eu-central-1")?;

    let claude = bedrock.anthropic().llm(Bedrock::EU_CLAUDE_HAIKU_5_5);
    println!("{}\n", claude.ask("What is a borrow checker?").await?);

    // Kimi K3 reasons before it answers, and its token limit counts the
    // reasoning too.
    let kimi = bedrock.openai().llm(Bedrock::KIMI_K3);
    let reply = kimi
        .request("What is a borrow checker?")
        .max_tokens(1024)
        .send()
        .await?;
    println!("{}", reply.text);
    Ok(())
}

/// The key from `AWS_BEARER_TOKEN_BEDROCK`, the variable AWS's own tools
/// use: the example reads it, the crate never does. Exits 2 with a usage
/// line when it is missing.
fn bedrock_key() -> String {
    std::env::var("AWS_BEARER_TOKEN_BEDROCK").unwrap_or_else(|_| {
        eprintln!("usage: AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example bedrock");
        std::process::exit(2)
    })
}
