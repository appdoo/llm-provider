# llm-provider-tools

Tools from outside for [llm-provider](https://github.com/appdoo/llm-provider) agents. The first module, `mcp`, turns the tools of any MCP server into `llm-provider` tools: one line adds a server's tools to an agent.

The crate is at 0.x, so a minor version may break the API.

## Install

```toml
[dependencies]
llm-provider = { version = "0.3", features = ["agent"] }
llm-provider-tools = "0.3"
```

## MCP servers as tools

A local server as a child process, and a remote server over Streamable HTTP with a bearer token ([`examples/fetch_agent.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider-tools/examples/fetch_agent.rs)):

```rust
use llm_provider::agent::{Session, Tools};
use llm_provider::Anthropic;
use llm_provider_tools::mcp::McpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(std::env::var("ANTHROPIC_API_KEY")?).llm("claude-sonnet-5-5");

    // A local server as a child process; its environment holds only what is passed.
    let fetch = McpServer::stdio("uvx", ["mcp-server-fetch"]).connect().await?;

    // A remote server over Streamable HTTP with a bearer token.
    let docs = McpServer::http("https://mcp.example.com/mcp")?
        .bearer(std::env::var("DOCS_TOKEN")?)
        .connect()
        .await?;

    let tools = Tools::new().with_all(fetch).with_all(docs);
    let mut agent = Session::new(claude, tools);
    println!("{}", agent.run_turn("Summarize https://www.rust-lang.org in two sentences.").await?.response);
    Ok(())
}
```

With a person in the loop:

```rust,ignore
let github = McpServer::stdio("github-mcp-server", ["stdio"])
    .env("GITHUB_TOKEN", token)
    .approve(|tool, input| {
        eprintln!("allow {tool} {input}? [y/N]");
        read_yes()
    })
    .prefix("github")                 // tools become github_create_issue, …
    .connect()
    .await?;
```

- `connect` starts the server (or opens the connection), runs `initialize`, lists all tools and keeps the session. Each MCP tool becomes one tool with the server's name, description and input schema; `names()` lists them.
- `prefix("x")` names every tool `x_<name>`. Two servers offering the same tool name collide in `Tools::with_all`, which panics naming the tool; a prefix on one of them is the remedy.
- `timeout` bounds each call and `connect` (60 s by default). A call's timeout, a protocol error or a lost connection answers the model with an error result; the agent's turn goes on. A lost connection is not reopened: later calls answer with an error too. A `connect` past the timeout fails with `Error::Timeout`.
- Model providers accept tool names of `[A-Za-z0-9_-]`, at most 64 characters. Every other character in `<prefix>_<name>` becomes `_` (`fs.read` is offered as `fs_read`; calls still reach the server as `fs.read`). A name over 64 characters, or two names that become the same, fails `connect` with an error naming the tools.
- `approve` runs on a blocking thread, so it may wait for a person.
- A result's text blocks are joined with blank lines; an image or audio block reads `[image <mime>]` / `[audio <mime>]`, a resource `[resource <uri>]` and its text; with no text, the structured result as JSON.
- Dropping the tools (and every tool taken from them) ends the session and stops a spawned server, also when the async runtime is dropped first.

## Security

- **Tool descriptions and results are untrusted.** They come from the server and reach the model as data; a server can write text meant to steer the model (prompt injection). Connect to servers you trust, and keep their output out of system prompts.
- **A stdio server runs with your privileges.** It is a program on your machine; you decide the exact command line. Its environment is cleared except `PATH`, `HOME`, `LANG`, `TERM` and the `env` entries you pass, so your other secrets do not reach it. Pass secrets through `env`, not the arguments: `Debug` prints the arguments.
- **Stdio servers: Linux and macOS.** The cleared environment is a Unix one; on Windows a server lacks the variables it needs to start. Windows is unsupported in 0.x.
- **Actions with effects need a person.** `approve` sees each call's tool name and input before anything is sent; on `false` the server never sees the call.
- **Credentials stay masked.** A bearer token and `env` values are held as `SecretString`; `Debug` does not print them.

CI runs `scripts/security.sh` on every push to `main`, every pull request and weekly: `cargo deny`, `osv-scanner`, `gitleaks` and `semgrep`. To report a vulnerability, use GitHub's private vulnerability reporting (the Security tab of [appdoo/llm-provider](https://github.com/appdoo/llm-provider/security)); please do not open a public issue for it.

## Examples

| Example | What it shows | Run |
|---|---|---|
| `fetch_agent` | Claude summarizes a page through `mcp-server-fetch` (needs `uvx` and `ANTHROPIC_API_KEY`) | `cargo run --example fetch_agent` |

`test_server` is the MCP server the tests spawn, not an example to copy.
