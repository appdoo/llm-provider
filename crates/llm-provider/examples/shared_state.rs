//! Tools that share state: a `#[tools]` block whose methods share one HTTP
//! client, next to the file tools `Files`, cut down with `Tools::only`.
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-… cargo run --example shared_state --features fs-tools
//! ```

use llm_provider::agent::{tools, Session, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;

/// Pages from the web, through one shared client.
struct Web {
    http: reqwest::Client,
}

/// Which page to fetch.
#[derive(Deserialize, JsonSchema)]
struct Page {
    /// The page's URL.
    url: String,
}

#[tools]
impl Web {
    /// Fetches a web page as text.
    #[tool]
    async fn fetch(&self, page: Page) -> Result<String, reqwest::Error> {
        self.http.get(&page.url).send().await?.text().await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(api_key()).llm(Anthropic::CLAUDE_HAIKU_5_5);

    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("notes.txt"),
        "Pages to check: https://example.com\n",
    )?;

    // `Files` and `Web` are each moved into one `Arc` their tools share.
    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new(dir.path())?))
        .only(["read", "grep"])
        .with_all(Web {
            http: reqwest::Client::new(),
        });

    let mut agent = Session::new(claude, tools);
    let turn = agent
        .run_turn("Read notes.txt, fetch the page it lists and tell me the page's title.")
        .await?;
    println!("{}", turn.response);
    let names: Vec<&str> = turn
        .tool_calls
        .iter()
        .map(|call| call.name.as_str())
        .collect();
    println!("tools called: {}", names.join(", "));
    Ok(())
}

/// The key from `ANTHROPIC_API_KEY`: the example reads it, the crate never
/// does. Exits 2 with a usage line when it is missing.
fn api_key() -> String {
    std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
        eprintln!("usage: ANTHROPIC_API_KEY=sk-ant-… cargo run --example shared_state --features fs-tools");
        std::process::exit(2)
    })
}
