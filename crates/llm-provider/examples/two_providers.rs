//! Two agents on two providers in one program: Claude plans, a local model
//! writes.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example two_providers --features agent -- [BASE_URL] [MODEL]
//! ```
//!
//! The local defaults are `http://localhost:11434/v1` (Ollama) and `llama3.2`.

use llm_provider::agent::{ConversationRuntime, Tools};
use llm_provider::{Anthropic, OpenAi};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key()).llm(Anthropic::CLAUDE_SONNET_5_5);
    // nosemgrep: rust.lang.security.args.args -- an example reads its own command line
    let mut args = std::env::args().skip(1);
    let base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:11434/v1".into());
    let local = OpenAi::at(base_url).llm(args.next().unwrap_or_else(|| "llama3.2".into()));

    let mut planner = ConversationRuntime::builder(claude, Tools::new())
        .system("You plan. Reply with a numbered list of at most five steps.")
        .build();
    let mut writer = ConversationRuntime::builder(local, Tools::new())
        .system("You write short release notes from a plan.")
        .build();

    let plan = planner
        .run_turn("Plan a 0.2.0 release of a small Rust library.")
        .await?;
    println!("--- plan (Claude) ---\n{}\n", plan.response);
    let notes = writer.run_turn(&plan.response).await?;
    println!("--- release notes (local) ---\n{}", notes.response);
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example two_providers --features agent -- [BASE_URL] [MODEL]");
        std::process::exit(2)
    })
}
