<div align="center">

# llm-provider

**A Rust API for LLMs.**

Anthropic, Amazon Bedrock, OpenAI and OpenAI-compatible servers (Ollama, vLLM): ask, stream, typed answers, tools and agents.

[![CI](https://github.com/appdoo/llm-provider/actions/workflows/ci.yml/badge.svg)](https://github.com/appdoo/llm-provider/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/llm-provider)](https://crates.io/crates/llm-provider) [![docs.rs](https://img.shields.io/docsrs/llm-provider)](https://docs.rs/llm-provider) [![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

0.x: breaks are listed in the [CHANGELOG](crates/llm-provider/CHANGELOG.md) with a before/after table.

</div>

```sh
cargo add llm-provider
cargo add tokio --features full
```

```rust,no_run
use llm_provider::Anthropic;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY")?;

    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    println!("{}", claude.ask("What is a borrow checker?").await?);
    Ok(())
}
```

The same call to a local server: `OpenAi::at("http://localhost:11434/v1")?.llm("qwen3")`; for Amazon Bedrock: `Bedrock::new(key, region)?` ([endpoints](https://docs.rs/llm-provider/latest/llm_provider/guide/ch07_endpoints/index.html)). Next: [stream](#2-stream), [a typed answer](#3-a-typed-answer), [a tool](#4-a-tool). Caching and cost, sessions, MCP and the rest: [what else is in it](#what-else-is-in-it).

## Building blocks

Six steps, each building on the one before, each a runnable program in [`examples/`](crates/llm-provider/examples/README.md) and a chapter of the [guide](https://docs.rs/llm-provider/latest/llm_provider/guide/index.html). Every block compiles as a doctest.

### 1. Ask

One question, one answer ([`examples/ask.rs`](crates/llm-provider/examples/ask.rs)). For Claude on Amazon Bedrock, the same protocol at another endpoint: `Bedrock::new(key, "eu-central-1")?.anthropic().llm(Bedrock::EU_CLAUDE_HAIKU_5_5)` ([`examples/bedrock.rs`](crates/llm-provider/examples/bedrock.rs)).

```rust
use llm_provider::Anthropic;

async fn ask(key: &str) -> Result<String, llm_provider::Error> {
    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    claude.ask("What is a borrow checker?").await
}
```
More: [Ask](https://docs.rs/llm-provider/latest/llm_provider/guide/ch01_ask/index.html).

### 2. Stream

The answer as it is written, with `.events()` for tool calls and token counts as well ([`examples/stream.rs`](crates/llm-provider/examples/stream.rs)). `futures::StreamExt` is your own import: `futures = "0.3"` in your dependencies.

```rust
use futures::StreamExt;
use llm_provider::Llm;

async fn story(claude: &Llm) -> Result<(), llm_provider::Error> {
    let mut text = claude.request("Tell a short story about a lighthouse.").stream();
    while let Some(delta) = text.next().await {
        print!("{}", delta?);
    }
    Ok(())
}
```
More: [Stream](https://docs.rs/llm-provider/latest/llm_provider/guide/ch02_stream/index.html).

### 3. A typed answer

With `schema`, the answer parses into your type and its doc comments are what the model reads ([`examples/typed_answer.rs`](crates/llm-provider/examples/typed_answer.rs)). The derives need `serde` and `schemars` in your own dependencies.

```rust
use llm_provider::{JsonSchema, Llm};
use serde::Deserialize;

/// What a text is about.
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

async fn summarize(claude: &Llm, text: &str) -> Result<Summary, llm_provider::Error> {
    claude.request(text).system("Be brief.").send_json::<Summary>().await
}
```
More: [A typed answer](https://docs.rs/llm-provider/latest/llm_provider/guide/ch03_typed_answer/index.html).

### 4. A tool

With `agent`, `#[tool]` turns an async function into a tool, here the type `UnixTime`, and the agent loop calls it when the model asks ([`examples/agent_tools.rs`](crates/llm-provider/examples/agent_tools.rs)):

```rust
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use llm_provider::agent::{tool, Session, Tools};
use llm_provider::Llm;

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string())
}

async fn timed(claude: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let mut agent = Session::new(claude, Tools::new().with(UnixTime));
    let turn = agent.run_turn("What is the Unix time now?").await?;
    Ok(turn.response)
}
```
More: [A tool](https://docs.rs/llm-provider/latest/llm_provider/guide/ch04_tool/index.html).

### 5. An advisor

An advisor is one more tool: Opus works, Fable advises, and it reads the whole conversation when Opus asks ([`examples/advisor.rs`](crates/llm-provider/examples/advisor.rs)):

```rust
use llm_provider::agent::{Advisor, Session, Tools, CONSULT_ADVISOR};
use llm_provider::Llm;

async fn advised(opus: Llm, stronger: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let tools = Tools::new().with(Advisor::new(stronger));
    let mut coder = Session::builder(opus, tools).system(CONSULT_ADVISOR).build();
    let turn = coder.run_turn("Why does median() return the wrong value?").await?;
    Ok(turn.response)
}
```
More: [An advisor](https://docs.rs/llm-provider/latest/llm_provider/guide/ch05_advisor/index.html).

### 6. Agents that ask each other

An agent becomes another agent's tool with `into_tool`; it keeps its own conversation ([`examples/delegation.rs`](crates/llm-provider/examples/delegation.rs)). Agents in separate tasks talk through a `#[tools]` block over a channel ([`examples/mailbox.rs`](crates/llm-provider/examples/mailbox.rs)).

```rust
use llm_provider::agent::{Session, Tools};
use llm_provider::Llm;

async fn lead(opus: Llm, haiku: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let researcher = Session::new(haiku, Tools::new())
        .into_tool("researcher", "Answers one factual question. Give it the question.");
    let mut lead = Session::new(opus, Tools::new().with(researcher));
    Ok(lead.run_turn("Ask the researcher who built the first lighthouse.").await?.response)
}
```
More: [Agents that ask each other](https://docs.rs/llm-provider/latest/llm_provider/guide/ch06_agents_that_ask_each_other/index.html).

## What else is in it

| Capability | The call | Guide |
|---|---|---|
| Other endpoints | `Bedrock::new(key, region)?.anthropic()`, `OpenAi::new(key)`, `OpenAi::at(url)?` | [7. Endpoints](https://docs.rs/llm-provider/latest/llm_provider/guide/ch07_endpoints/index.html) |
| Prompt caching, cost | `.cache(CacheTtl::OneHour)` on a request or the session builder, on by default in the agent loop; `usage.cost(&Prices::per_million(3.0, 15.0))` | [8. Caching and cost](https://docs.rs/llm-provider/latest/llm_provider/guide/ch08_caching_and_cost/index.html) |
| The agent loop, a bad or hung tool call | `Session::new(llm, tools).run_turn(..)`; a failed tool call becomes an error result for the model, `.tool_timeout(..)` | [9. The agent loop](https://docs.rs/llm-provider/latest/llm_provider/guide/ch09_agent_loop/index.html) |
| Store and restore a session | `.events(tx)`, `.restore(events)` | [10. Store and restore](https://docs.rs/llm-provider/latest/llm_provider/guide/ch10_store_and_restore/index.html) |
| Compaction | `.compact_when(tokens, callback)` | [11. Compaction](https://docs.rs/llm-provider/latest/llm_provider/guide/ch11_compaction/index.html) |
| Cancellation | `run_turn_cancellable`, `send_cancellable` | [12. Cancellation](https://docs.rs/llm-provider/latest/llm_provider/guide/ch12_cancellation/index.html) |
| Sampling, timeouts, retry | `.sampling(Sampling::new())`, `.timeouts(ProviderOptions)`, `.retry(RetryPolicy)` | [13. Sampling, timeouts, retry](https://docs.rs/llm-provider/latest/llm_provider/guide/ch13_sampling_timeouts_retry/index.html) |
| Models from configuration, the explicit form | `Llm::builder(model)`, `OpenAi::builder(url)?` | [14. Configuration](https://docs.rs/llm-provider/latest/llm_provider/guide/ch14_configuration/index.html) |
| Sandboxed file tools | `Files::new(Sandbox::new(root)?)` (`fs-tools`) | [15. File tools](https://docs.rs/llm-provider/latest/llm_provider/guide/ch15_file_tools/index.html) |
| Embeddings, batches | `OpenAi::embedder(model)`, `Embed::embed`; `Anthropic::batches(model)` (`batch`) | [16. Embeddings and batches](https://docs.rs/llm-provider/latest/llm_provider/guide/ch16_embeddings_and_batches/index.html) |
| MCP servers as tools | `McpServer::stdio(..).connect()` in the crate `llm-provider-tools` | [17. MCP servers](https://docs.rs/llm-provider/latest/llm_provider/guide/ch17_mcp_servers/index.html) |
| Your own provider | `core::Transport`, `core::decode` | [18. Your own provider](https://docs.rs/llm-provider/latest/llm_provider/guide/ch18_own_provider/index.html) |

## Cargo features

| Feature | Adds |
|---|---|
| default | the endpoints `Anthropic`, `OpenAi` and `Bedrock`; the models `Llm` (`ask`, `request`, `send_as`) and `Embedder` (`embed`) |
| `schema` | `send_json` and `ResponseSchema::of`: the answer schema derived from your type |
| `agent` | the agent loop `Session`, `#[tool]`, `#[tools]`, `Advisor` (turns on `schema`) |
| `fs-tools` | the sandboxed file tools `Files`: read, write, edit, grep, glob (turns on `agent`) |
| `batch` | Anthropic message batches |

0.x: a minor version may break the API; every break is in the [CHANGELOG](crates/llm-provider/CHANGELOG.md) with a before/after table.

## What the caller decides

- **Credentials:** passed in as an `AuthMethod`, never looked up; `Debug` masks them.
- **Where requests go:** each endpoint names its address. `Anthropic::new(key)` sends to `https://api.anthropic.com`, `OpenAi::new(key)` to `https://api.openai.com/v1`, `OpenAi::at(url)` to the server you name, and `Bedrock::new(key, region)` to `https://bedrock-runtime.{region}.amazonaws.com`, under `/anthropic` or `/openai/v1`.
- **Timeouts and retry:** `ProviderOptions` (30 s connect, 90 s silence) and `RetryPolicy` (4 attempts).
- **The sandbox:** file tools resolve every path inside one root; no guard against races or hard links.
- **Model output is untrusted:** a tool validates its input before acting on it.

## Tested with

Verified live: Claude Haiku, Sonnet, Opus and Fable 5.x on the Anthropic API, Claude on Amazon Bedrock and Kimi K3 on Bedrock, each with ask, stream, `send_json` and a tool turn: `cargo run --example verify --all-features` (your own `ANTHROPIC_API_KEY` or `AWS_BEARER_TOKEN_BEDROCK`; `--route anthropic|bedrock|kimi|ollama`, `--model <id>` select a subset).

## Security

CI runs `scripts/security.sh` on every push to `main`, every pull request and weekly: `cargo deny` (advisories, licences, bans, sources), `osv-scanner`, `gitleaks` and `semgrep`. To report a vulnerability, use GitHub's private vulnerability reporting (the Security tab of [appdoo/llm-provider](https://github.com/appdoo/llm-provider/security)). Please do not open a public issue for it.

## License

MIT. See `LICENSE`.
