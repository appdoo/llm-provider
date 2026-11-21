//! An agent with the tools of the MCP fetch server: Claude reads a web page
//! through `mcp-server-fetch` and summarizes it.
//!
//! Needs `uvx` (from `uv`) on the `PATH` and `ANTHROPIC_API_KEY` set; the
//! example reads the key, the crate never reads an environment variable.
//!
//! ```text
//! cargo run --example fetch_agent
//! ```

use llm_provider::agent::{Session, Tools};
use llm_provider::Anthropic;
use llm_provider_tools::mcp::McpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else {
        eprintln!("usage: ANTHROPIC_API_KEY=… cargo run --example fetch_agent");
        std::process::exit(2);
    };
    let claude = Anthropic::new(key).llm("claude-sonnet-5-5");

    // A local server as a child process; its environment holds only what is passed.
    let fetch = McpServer::stdio("uvx", ["mcp-server-fetch"])
        .connect()
        .await?;

    let mut agent = Session::new(claude, Tools::new().with_all(fetch));
    let turn = agent
        .run_turn("Summarize https://www.rust-lang.org in two sentences.")
        .await?;
    println!("{}", turn.response);
    Ok(())
}
