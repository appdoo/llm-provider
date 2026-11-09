//! A lead that delegates: two agents become tools with `into_tool`, and an
//! `Advisor` sits beside them. The researcher answers from its own
//! knowledge; the coder edits files in a scratch directory.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example delegation --features fs-tools
//! ```

use llm_provider::agent::{Advisor, ConversationRuntime, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::Anthropic;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = api_key();
    let model = |id: &str| Anthropic::new(key.as_str(), id);
    let dir = tempfile::tempdir()?;
    let calendar = dir.path().join("calendar.py");
    std::fs::write(&calendar, "def days_in_february(year):\n    return 28\n")?;

    let researcher = ConversationRuntime::builder(model(Anthropic::CLAUDE_HAIKU_5_5), Tools::new())
        .system("You answer one factual question briefly and precisely.")
        .build()
        .into_tool(
            "researcher",
            "Answers one factual question. Give it the question.",
        );
    let coder_tools = Tools::new().with_all(Files::new(Sandbox::new(dir.path())?));
    let coder = ConversationRuntime::builder(model(Anthropic::CLAUDE_SONNET_5_5), coder_tools)
        .system("You edit files to carry out one task.")
        .build()
        .into_tool("coder", "Edits files in the project. Give it one task.");

    let tools = Tools::new()
        .with(researcher)
        .with(coder)
        .with(Advisor::new(model(Anthropic::CLAUDE_FABLE_5_1)).max_uses(1));
    let mut lead = ConversationRuntime::builder(model(Anthropic::CLAUDE_OPUS_5_5), tools)
        .system("You lead: ask the researcher for facts, give the coder the edit, then verify.")
        .max_iterations(15)
        .build();

    let turn = lead
        .run_turn("calendar.py ignores leap years. Have it fixed using the Gregorian rules.")
        .await?;
    for call in &turn.tool_calls {
        println!("[{}] {}", call.name, call.input);
    }
    println!(
        "\n{}\n\n--- calendar.py ---\n{}",
        turn.response,
        std::fs::read_to_string(&calendar)?
    );
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!(
            "usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example delegation --features fs-tools"
        );
        std::process::exit(2)
    })
}
