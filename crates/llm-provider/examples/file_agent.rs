//! An agent on a local model that may read and edit the files of one
//! directory; its streamed text goes to `on_delta`.
//!
//! ```sh
//! cargo run --example file_agent --features fs-tools -- [BASE_URL] [MODEL]
//! ```
//!
//! The defaults are `http://localhost:11434/v1` (Ollama) and `llama3.2`; the
//! model has to support tool calls.

use std::io::Write as _;

use llm_provider::agent::{ConversationRuntime, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::OpenAiCompatible;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let base_url = args
        .next()
        .unwrap_or_else(|| "http://localhost:11434/v1".into());
    let model = OpenAiCompatible::new(base_url, args.next().unwrap_or_else(|| "llama3.2".into()));

    // A scratch directory with one file full of typos.
    let dir = tempfile::tempdir()?;
    let readme = dir.path().join("README.md");
    std::fs::write(&readme, "# Exmaple\n\nThis is a smal projcet.\n")?;

    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new(dir.path())?))
        .only(["read", "edit"]);
    let mut agent = ConversationRuntime::builder(model, tools)
        .system("You fix typos in the files you are given. Use the read and edit tools.")
        .max_iterations(10)
        .on_delta(|text| print!("{text}"))
        .build();

    let turn = agent.run_turn("Fix the typos in README.md.").await?;
    std::io::stdout().flush()?;
    println!(
        "\n\n{} tool calls; README.md now:\n{}",
        turn.tool_calls.len(),
        std::fs::read_to_string(&readme)?
    );
    Ok(())
}
