//! A writer with two critics. Each critic is an `Advisor` with its own
//! persona; the writer hears both and names where they disagree.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example critics --features agent
//! ```

use llm_provider::agent::{Advisor, Session, Tools};
use llm_provider::Anthropic;

/// The writer's persona.
const DEMOAUTHOR1: &str = "You are demoauthor1, a writer of short fiction. \
Before you revise, ask both critics, demoauthor2 and editor. Where they \
disagree, say so in one line and decide. Then give the revised scene.";

/// The first critic's persona.
const DEMOAUTHOR2: &str = "You are demoauthor2, a critic of plot. Judge \
plausibility and logic; say what to cut. Three sentences at most.";

/// The second critic's persona.
const EDITOR: &str = "You are an editor. Judge rhythm, clarity and \
repetition; say what to tighten. Three sentences at most.";

/// The scene to revise.
const SCENE: &str = "The lighthouse keeper saw the ship. He saw it was \
sinking. He ran down the stairs, all two hundred of them, and rowed out \
alone in the storm, and he saved everyone, all forty of them, by himself.";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = api_key();
    let critic = || Anthropic::new(key.as_str()).llm(Anthropic::CLAUDE_SONNET_5_5);
    let demoauthor2 = Advisor::new(critic())
        .name("demoauthor2")
        .description("Critic: plot logic, plausibility, what to cut.")
        .system(DEMOAUTHOR2);
    let editor = Advisor::new(critic())
        .name("editor")
        .description("Critic: rhythm, clarity, repetition.")
        .system(EDITOR);

    let writer = Anthropic::new(key.as_str()).llm(Anthropic::CLAUDE_OPUS_5_5);
    let tools = Tools::new().with(demoauthor2).with(editor);
    let mut demoauthor1 = Session::builder(writer, tools).system(DEMOAUTHOR1).build();

    let turn = demoauthor1
        .run_turn(&format!("Revise this scene:\n\n{SCENE}"))
        .await?;
    for call in &turn.tool_calls {
        println!("--- {} ---\n{}\n", call.name, call.output.content);
    }
    println!("--- demoauthor1 ---\n{}", turn.response);
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example critics --features agent");
        std::process::exit(2)
    })
}
