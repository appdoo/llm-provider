<div align="center">

# llm-provider

**A small, pure-Rust toolbox for LLM agents, from one completion to a tool-using agent loop.**

[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](crates/llm-provider/CHANGELOG.md)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
[![Status](https://img.shields.io/badge/status-experimental-orange?style=flat-square)](#license)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-B7410E?style=flat-square&logo=rust&logoColor=white)](Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-424%20passing-brightgreen?style=flat-square)](#tested-with)

[📑 Manual](#manual) • [🧪 Examples](crates/llm-provider/examples/) • [📋 Changelog](crates/llm-provider/CHANGELOG.md) • [🔒 Security](#security)

</div>

## Crates

| Crate | What it is | Status |
|---|---|---|
| [`llm-provider`](crates/llm-provider) | the endpoints and their models, typed answers, the agent loop and the sandboxed file tools | experimental, 0.1 |
| [`llm-provider-macros`](crates/llm-provider-macros) | the `#[tool]` and `#[tools]` macros, used through `llm-provider` | experimental, 0.1, versioned with `llm-provider` |
| [`llm-provider-tools`](crates/llm-provider-tools) | tools from outside: MCP servers as `llm-provider` tools | experimental, 0.1 |

- **A small front API.** An endpoint hands out a model, and the model answers: `Anthropic::new(key).llm(model).ask("…")`.
- **Two protocols, any address.** The Anthropic Messages API in depth (prompt caching, OAuth, structured outputs, batches), at Anthropic or on Amazon Bedrock, and every server that speaks the OpenAI Chat Completions API: OpenAI, Ollama, vLLM, LM Studio, llama.cpp, `OpenRouter`, Groq, Mistral, Bedrock's `/openai` route; embeddings from the same servers.
- **Agents from a few pieces that compose.** Tools, agents as tools, and advisors.

It is built for the author's own projects; issues and pull requests are welcome, with no guaranteed support.

## Install

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["agent"] }
```

| Feature | Adds |
|---|---|
| default | the endpoints `Anthropic`, `OpenAi` and `Bedrock`; the models `Llm` (`ask`, `request`, `send_as`) and `Embedder` (`embed`) |
| `schema` | `send_json` and `ResponseSchema::of`: the answer schema derived from your type |
| `agent` | the agent loop `ConversationRuntime`, `#[tool]`, `#[tools]`, `Advisor` (turns on `schema`) |
| `fs-tools` | the sandboxed file tools `Files`: read, write, edit, grep, glob (turns on `agent`) |
| `batch` | Anthropic message batches |

## Building blocks

Six rungs, each a small step from the one before and each a runnable program in `crates/llm-provider/examples/`. Every block below compiles as a doctest. The frames only show what a line needs; no block is called.

### 1. Ask

One question, one answer ([`examples/ask.rs`](crates/llm-provider/examples/ask.rs)). For Claude on Amazon Bedrock, the same protocol at another endpoint: `Bedrock::new(key, "eu-central-1").anthropic().llm(Bedrock::EU_CLAUDE_HAIKU_5_5)` ([`examples/bedrock.rs`](crates/llm-provider/examples/bedrock.rs)).

```rust
use llm_provider::Anthropic;

async fn ask(key: &str) -> Result<String, llm_provider::Error> {
    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    claude.ask("What is a borrow checker?").await
}
```

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

### 4. A tool

With `agent`, `#[tool]` turns an async function into a tool, here the type `UnixTime`, and the agent loop calls it when the model asks ([`examples/agent_tools.rs`](crates/llm-provider/examples/agent_tools.rs)):

```rust
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use llm_provider::agent::{tool, ConversationRuntime, Tools};
use llm_provider::Llm;

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string())
}

async fn timed(claude: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let mut agent = ConversationRuntime::new(claude, Tools::new().with(UnixTime));
    let turn = agent.run_turn("What is the Unix time now?").await?;
    Ok(turn.response)
}
```

### 5. An advisor

An advisor is one more tool: Opus works, Fable advises, and it reads the whole conversation when Opus asks ([`examples/advisor.rs`](crates/llm-provider/examples/advisor.rs)):

```rust
use llm_provider::agent::{Advisor, ConversationRuntime, Tools, CONSULT_ADVISOR};
use llm_provider::Llm;

async fn advised(opus: Llm, stronger: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let tools = Tools::new().with(Advisor::new(stronger));
    let mut coder = ConversationRuntime::builder(opus, tools).system(CONSULT_ADVISOR).build();
    let turn = coder.run_turn("Why does median() return the wrong value?").await?;
    Ok(turn.response)
}
```

### 6. Agents that ask each other

An agent becomes another agent's tool with `into_tool`; it keeps its own conversation ([`examples/delegation.rs`](crates/llm-provider/examples/delegation.rs)). Agents in separate tasks talk through a `#[tools]` block over a channel ([`examples/mailbox.rs`](crates/llm-provider/examples/mailbox.rs)).

```rust
use llm_provider::agent::{ConversationRuntime, Tools};
use llm_provider::Llm;

async fn lead(opus: Llm, haiku: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let researcher = ConversationRuntime::new(haiku, Tools::new())
        .into_tool("researcher", "Answers one factual question. Give it the question.");
    let mut lead = ConversationRuntime::new(opus, Tools::new().with(researcher));
    Ok(lead.run_turn("Ask the researcher who built the first lighthouse.").await?.response)
}
```

## The explicit form

An endpoint built by hand reads left to right: protocol, address, credential, model name. Every model made from one endpoint shares its connection pool.

```rust
use llm_provider::{Llm, OpenAi};

fn qwen(key: &str) -> Llm {
    OpenAi::builder("http://gpu-box:8000/v1") // protocol, address
        .credential(key)                     // credential
        .build()
        .llm("qwen3")                        // model name
}
```

## From configuration

When the protocol, the address or the model is known only at run time (from a configuration file or a command-line flag), `Llm::builder(model)` and `Embedder::builder(model)` take them as values. Each `build()` makes its own endpoint, so models built this way do not share a connection pool; the typed form above does.

```rust
use llm_provider::{Llm, ModelBuilder, Protocol};
use secrecy::SecretString;

fn models(key: SecretString, text: &str) -> Result<(Llm, Llm), llm_provider::Error> {
    let qwen = Llm::builder("qwen3")             // the model name
        .protocol(Protocol::OpenAi)              // the protocol
        .endpoint("http://gpu-box:8000/v1")      // the address
        .api_key(key.clone())                    // the credential, always set in code
        .build()?;

    // The same values from a file; the credential never comes from it.
    let opus = serde_json::from_str::<ModelBuilder<Llm>>(text)?.api_key(key).build()?;
    Ok((qwen, opus))
}
```

A file names `protocol` (`anthropic` or `openai`), `endpoint`, `region` (Amazon Bedrock), `model`, `max_attempts`, `connect_timeout_secs` and `stream_idle_timeout_secs`:

```json
{ "protocol": "anthropic", "region": "eu-central-1", "model": "eu.anthropic.claude-opus-5-5" }
```

## Examples

Each file in `crates/llm-provider/examples/` is a complete program. The Claude examples read the key from `ANTHROPIC_API_KEY` (the example does; the crate never reads an environment variable) and exit 2 with a usage line without it. The local-model examples take `[BASE_URL] [MODEL]`, default `http://localhost:11434/v1` and `llama3.2`.

| Example | What it shows | Run |
|---|---|---|
| `ask` | one question, then a request with a system prompt and a token limit | `cargo run --example ask` |
| `stream` | a story token by token, then the token usage from the event stream | `cargo run --example stream` |
| `typed_answer` | `send_as` with a hand-written schema, `send_json` with a derived one | `cargo run --example typed_answer --features schema` |
| `local_model` | a local OpenAI-compatible server, no key | `cargo run --example local_model` |
| `agent_tools` | an agent with two `#[tool]` functions | `cargo run --example agent_tools --features agent` |
| `shared_state` | a `#[tools]` block sharing one HTTP client, next to `Files` cut down with `only` | `cargo run --example shared_state --features fs-tools` |
| `tool_by_hand` | a `Tool` implemented by hand, named at run time | `cargo run --example tool_by_hand --features agent` |
| `file_agent` | a local model that fixes typos with the file tools, streamed | `cargo run --example file_agent --features fs-tools` |
| `advisor` | Opus 5.5 fixes a bug, Fable 5.1 advises; advice and tally printed | `cargo run --example advisor --features fs-tools` |
| `critics` | a writer with two critics whose disagreement stays visible | `cargo run --example critics --features agent` |
| `delegation` | a lead that delegates to two agents (`into_tool`) and asks an advisor | `cargo run --example delegation --features fs-tools` |
| `mailbox` | two agents in two tasks talking over a channel | `cargo run --example mailbox --features agent` |
| `sandbox` | the file tools called directly; a read outside the root fails (offline) | `cargo run --example sandbox --features fs-tools` |
| `two_providers` | Claude plans, a local model writes | `cargo run --example two_providers --features agent` |

## Tools from outside

`llm-provider-tools` turns the tools of any MCP server, local over stdio or remote over Streamable HTTP, into `llm-provider` tools. Its install line, example and limits are in [its README](crates/llm-provider-tools/README.md).

## Manual

The manual is the crate documentation: `cargo doc --open --all-features` (docs.rs once published).

- `llm_provider` (crate root): the levels and their features.
- `provider`: the endpoints `Anthropic`, `OpenAi` and `Bedrock`, the models `Llm` and `Embedder`, protocols, credentials, typed answers and the JSON Schema subset (`Request::send_as`), timeouts (`ProviderOptions`), retry (`RetryPolicy`), size caps.
- `agent`: the turn loop, `#[tool]` and `#[tools]`, `Advisor` and `CONSULT_ADVISOR`, agents as tools (`ConversationRuntime::into_tool`), turn limits (`RuntimeBuilder`).
- `fs_tools`: the file tools (`Files`) and what the `Sandbox` covers.

## Tested with

Measured live on 2026-11-10 with `cargo run --example verify --all-features`
(113.2 s). Each check is one small request: **ask** a short answer, **stream**
it, **send_json** a typed answer (`{country, city}` for France), and a **tool
turn** (the agent calls an `add` tool for 17 + 25 and answers 42).

| Route | Model | ask | stream | send_json | tool turn |
|---|---|---|---|---|---|
| Anthropic API | `claude-haiku-5-5` | ✅ | ✅ | ✅ | ✅ |
| Anthropic API | `claude-sonnet-5-5` | ✅ | ✅ | ✅ | ✅ |
| Anthropic API | `claude-opus-5-5` | ✅ | ✅ | ✅ | ✅ |
| Anthropic API | `claude-fable-5-1` | ✅ | ✅ | ✅ | ✅ |
| Bedrock `/anthropic` | `eu.anthropic.claude-haiku-5-5` | ✅ | ✅ | ✅ | ✅ |
| Bedrock `/anthropic` | `eu.anthropic.claude-sonnet-5-5` | ✅ | ✅ | ✅ | ✅ |
| Bedrock `/anthropic` | `eu.anthropic.claude-opus-5-5` | ✅ | ✅ | ✅ | ✅ |
| Bedrock `/openai` | `global.moonshotai.kimi-k3` | ✅ | ✅ | ✅ | ✅ |
| Ollama | `llama3.2`, `qwen3` | not run yet | | | |

Re-measure with your own keys (`ANTHROPIC_API_KEY`, `AWS_BEARER_TOKEN_BEDROCK`)
or a subset: `--route anthropic|bedrock|kimi|ollama`, `--model <id>`.

## What the caller decides

- **Credentials:** passed in as an `AuthMethod`, never looked up; `Debug` masks them.
- **Where requests go:** each endpoint names its address. `Anthropic::new(key)` sends to `https://api.anthropic.com`, `OpenAi::new(key)` to `https://api.openai.com/v1`, `OpenAi::at(url)` to the server you name, and `Bedrock::new(key, region)` to `https://bedrock-runtime.{region}.amazonaws.com`, under `/anthropic` or `/openai/v1`.
- **Timeouts and retry:** `ProviderOptions` (30 s connect, 90 s silence) and `RetryPolicy` (4 attempts).
- **The sandbox:** file tools resolve every path inside one root; no guard against races or hard links.
- **Model output is untrusted:** a tool validates its input before acting on it.

## Security

CI runs `scripts/security.sh` on every push, every pull request and weekly: `cargo deny` (advisories, licences, bans, sources), `osv-scanner`, `gitleaks` and `semgrep`.

To report a vulnerability, use GitHub's private vulnerability reporting once the repository is public. Until then, report it privately to the repository owner. Please do not open a public issue for it.

> [!WARNING]
> llm-provider is an experimental testbed, pre-1.0. Releases contain breaking changes; the CHANGELOG says what changed and how to move.

## License

MIT. See `LICENSE`.
