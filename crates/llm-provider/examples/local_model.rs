//! One question to a local model through its OpenAI-compatible endpoint,
//! with no key.
//!
//! ```sh
//! cargo run --example local_model -- [BASE_URL] [MODEL]
//! ```
//!
//! The defaults are `http://localhost:11434/v1` (Ollama) and `llama3.2`.

use llm_provider::OpenAi;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // nosemgrep: rust.lang.security.args.args -- an example reads its own command line
    let mut args = std::env::args().skip(1);
    let base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:11434/v1".into());
    let model = args.next().unwrap_or_else(|| "llama3.2".into());

    let local = OpenAi::at(base_url)?.llm(model);
    let answer = local.ask("What is a borrow checker?").await?;
    println!("{answer}");
    Ok(())
}
