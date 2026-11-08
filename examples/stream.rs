//! A story from Claude, printed token by token, then the token usage of a
//! second request, read from its full event stream.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example stream
//! ```

use llm_provider::{Anthropic, Error};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key(), "claude-haiku-5-5");

    print_story(&claude).await?;
    print_usage(&claude).await?;
    Ok(())
}

/// Prints a short story as its text deltas arrive.
async fn print_story(_claude: &Anthropic) -> Result<(), Error> {
    todo!()
}

/// Prints the token usage of a second request.
async fn print_usage(_claude: &Anthropic) -> Result<(), Error> {
    todo!()
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example stream");
        std::process::exit(2)
    })
}
