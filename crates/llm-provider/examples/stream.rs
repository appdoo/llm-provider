//! A story from Claude, printed token by token, then the token usage of a
//! second request, read from its full event stream.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example stream
//! ```

use std::error::Error;
use std::io::Write;

use futures::StreamExt;
use llm_provider::provider::Usage;
use llm_provider::{Anthropic, StreamEvent};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let claude = Anthropic::new(api_key(), Anthropic::CLAUDE_HAIKU_5_5);

    print_story(&claude).await?;
    print_usage(&claude).await?;
    Ok(())
}

/// Prints a short story as its text deltas arrive.
async fn print_story(claude: &Anthropic) -> Result<(), Box<dyn Error>> {
    let mut text = claude
        .request("Tell a short story about a lighthouse, in four sentences.")
        .stream();

    while let Some(delta) = text.next().await {
        print!("{}", delta?);
        std::io::stdout().flush()?;
    }

    println!("\n");
    Ok(())
}

/// Prints the token usage of a second request, whose usage events add up.
async fn print_usage(claude: &Anthropic) -> Result<(), Box<dyn Error>> {
    let mut events = claude.request("What is a borrow checker?").events();

    let mut usage = Usage::default();
    while let Some(event) = events.next().await {
        if let StreamEvent::Usage(more) = event? {
            usage += more;
        }
    }

    println!(
        "tokens: {} in, {} out",
        usage.input_tokens, usage.output_tokens
    );
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example stream");
        std::process::exit(2)
    })
}
