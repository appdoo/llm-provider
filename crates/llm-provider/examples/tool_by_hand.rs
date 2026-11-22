//! A tool written by hand: its name is decided at run time, so it
//! implements `Tool` itself instead of using `#[tool]`.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example tool_by_hand --features agent -- [TOOL_NAME]
//! ```

use async_trait::async_trait;
use llm_provider::agent::{CallContext, Session, Tool, ToolOutput, Tools};
use llm_provider::provider::ToolDefinition;
use llm_provider::Anthropic;
use serde_json::{json, Value};

/// Answers every call with the same text, under a name given at run time.
struct Echo {
    name: String,
    reply: String,
}

#[async_trait]
impl Tool for Echo {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            self.name.clone(),
            format!("Answers {}.", self.reply),
            json!({ "type": "object", "properties": {} }),
        )
    }

    async fn call(&self, _input: Value, context: &CallContext<'_>) -> ToolOutput {
        let seen = context.transcript.messages.len();
        ToolOutput::ok(format!("{} (messages so far: {seen})", self.reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key()).llm(Anthropic::CLAUDE_HAIKU_5_5);

    // nosemgrep: rust.lang.security.args.args -- an example reads its own command line
    let name = std::env::args().nth(1).unwrap_or_else(|| "echo".into());
    let prompt = format!("Call the {name} tool once and tell me what it answered.");

    let tools = Tools::new().with(Echo {
        name,
        reply: "pong".into(),
    });
    let mut agent = Session::new(claude, tools);
    let turn = agent.run_turn(&prompt).await?;
    println!("{}", turn.response);
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example tool_by_hand --features agent -- [TOOL_NAME]");
        std::process::exit(2)
    })
}
