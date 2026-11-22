<div align="center">

# llm-provider

**A Rust API for LLMs.**

Anthropic, Amazon Bedrock, OpenAI and OpenAI-compatible servers (Ollama, vLLM): ask, stream, typed answers, tools and agents.

[![CI](https://github.com/appdoo/llm-provider/actions/workflows/ci.yml/badge.svg)](https://github.com/appdoo/llm-provider/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-provider)](https://crates.io/crates/llm-provider)
[![docs.rs](https://img.shields.io/docsrs/llm-provider)](https://docs.rs/llm-provider)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)

[Manual](#manual) • [Examples](crates/llm-provider/examples/) • [Changelog](crates/llm-provider/CHANGELOG.md) • [Security](#security)

</div>

- Prompt caching, on by default in the agent loop; `Usage::cost` gives the cost of a call or a turn.
- Typed answers (`send_json`), tools from a plain function (`#[tool]`), MCP servers as tools.
- A bad tool call or a hung tool becomes an error result for the model; a failed turn returns its work.
- `agent`, `schema`, `fs-tools` and `batch` are opt-in features.

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

Then stream, typed answers, tools and agents: [Building blocks](#building-blocks).

0.x: a minor version may break the API; every break is in the [CHANGELOG](crates/llm-provider/CHANGELOG.md) with a before/after table.

## Crates

| Crate | What it is |
|---|---|
| [`llm-provider`](crates/llm-provider) | the endpoints and their models, typed answers, the agent loop and the sandboxed file tools |
| [`llm-provider-macros`](crates/llm-provider-macros) | the `#[tool]` and `#[tools]` macros, used through `llm-provider` |
| [`llm-provider-tools`](crates/llm-provider-tools) | tools from outside: MCP servers as `llm-provider` tools |

Your own provider: `core` is the kit the providers are built on (`Transport`, `decode`); see [`examples/own_provider.rs`](crates/llm-provider/examples/own_provider.rs).

## Features

| Feature | Adds |
|---|---|
| default | the endpoints `Anthropic`, `OpenAi` and `Bedrock`; the models `Llm` (`ask`, `request`, `send_as`) and `Embedder` (`embed`) |
| `schema` | `send_json` and `ResponseSchema::of`: the answer schema derived from your type |
| `agent` | the agent loop `Session`, `#[tool]`, `#[tools]`, `Advisor` (turns on `schema`) |
| `fs-tools` | the sandboxed file tools `Files`: read, write, edit, grep, glob (turns on `agent`) |
| `batch` | Anthropic message batches |

## Building blocks

Six steps, each building on the one before and each a runnable program in `crates/llm-provider/examples/`. Every block below compiles as a doctest (`no_run`).

### 1. Ask

One question, one answer ([`examples/ask.rs`](crates/llm-provider/examples/ask.rs)). For Claude on Amazon Bedrock, the same protocol at another endpoint: `Bedrock::new(key, "eu-central-1")?.anthropic().llm(Bedrock::EU_CLAUDE_HAIKU_5_5)` ([`examples/bedrock.rs`](crates/llm-provider/examples/bedrock.rs)).

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

## Store and restore a session

A session sends every model call, compaction and turn end as a `SessionEvent` over a channel you give it; the crate stores nothing. Each event serializes with serde, carries the session id and a `seq`, and holds the call's own `Usage`: price it with `usage.cost(&prices)`. To continue a session, hand the stored events back to `restore`; the crate folds them ([`examples/session_store.rs`](crates/llm-provider/examples/session_store.rs) shows a file as the database and compaction with a summarizer through `compact_when`).

```rust,no_run
use llm_provider::agent::{Session, SessionEvent, Tools};
use llm_provider::Llm;

async fn store_and_restore(llm: Llm, stored: Vec<SessionEvent>) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<SessionEvent>(64);
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let line = serde_json::to_string(&event).expect("an event serializes");
            println!("{line}"); // the app's database goes here
        }
    });

    let mut agent = Session::builder(llm, Tools::new())
        .restore(stored) // the events your database returned
        .events(tx)
        .build();
    agent.run_turn("Where were we?").await?;
    Ok(())
}
```

## Caching and sampling

A long prefix that stays the same between calls (a handbook in the system prompt, the tool list, the conversation so far) can be cached: `cache` marks the last tool, the system prompt and the last message, and the provider reads them back at a tenth of the input price. It works at Anthropic and on Amazon Bedrock; OpenAI caches on its own and ignores the marks. The agent loop caches by default for five minutes (`Session::builder(..).cache(CacheTtl::OneHour)` or `.no_cache()`). A cache write costs 1.25 times the input price (2 times for one hour) and a read 0.1 times, so the cache pays when the next call comes within its lifetime: right for a tool loop, wrong for a chat with minutes between turns, which should use `CacheTtl::OneHour` or `.no_cache()`. Below the model's minimum size the provider caches nothing and says nothing, so check `Usage` ([`examples/cache.rs`](crates/llm-provider/examples/cache.rs)).

`sampling` sets temperature, top-p, seed and stop sequences. The values go to the provider as they are; a model that does not take one answers with its own error (Claude Sonnet 5.5, for one: "`temperature` is deprecated for this model", `ErrorKind::InvalidRequest`).

```rust
use llm_provider::{CacheTtl, Llm, Sampling};

async fn answer(model: &Llm, handbook: &str, question: &str) -> Result<String, llm_provider::Error> {
    let reply = model
        .request(question)
        .system(handbook)
        .cache(CacheTtl::OneHour)
        .sampling(Sampling::new().temperature(0.2)) // where the model takes it
        .send()
        .await?;
    let usage = reply.usage;
    println!("cache: written {} (1h {}), read {}",
        usage.cache_creation_input_tokens,
        usage.cache_creation_1h_input_tokens,
        usage.cache_read_input_tokens);
    Ok(reply.text)
}
```

A model call's `Usage` converts to dollars with `Prices`: the input and output price per million tokens, the cache prices derived from the input price (write 1.25 times, or 2 times for one hour; read 0.1 times) and each overridable. The crate ships no price table, since prices change. Amazon Bedrock bills the `eu.`, `us.` and `apac.` inference profiles at 1.10 times the global list price in every category (verified against an AWS bill); `regional(1.10)` applies it. A `Usage` is per call and `TurnSummary::usage` adds a turn's calls up, so costs add.

```rust
use llm_provider::{Prices, Usage};

fn dollars(usage: &Usage) -> f64 {
    // Claude Sonnet on Bedrock via an `eu.` profile: list price (2026-11-21) × 1.10.
    let sonnet = Prices::per_million(3.0, 15.0).regional(1.10);
    usage.cost(&sonnet)
}
```

## Cancellation

`run_turn_cancellable` and `send_cancellable` stop on a `CancellationToken`. A cancelled turn leaves the history as it was and hands back its text and the tool calls that finished.

```rust
use llm_provider::agent::{Session, SessionError, Tools};
use tokio_util::sync::CancellationToken;

async fn plan(agent: &mut Session<Tools>, token: &CancellationToken) {
    match agent.run_turn_cancellable("Plan the refactor.", token).await {
        Err(SessionError::Interrupted { turn, .. }) => {
            println!("cut after {} tool calls: {}", turn.tool_calls.len(), turn.response);
        }
        other => println!("{other:?}"),
    }
}
```

## The explicit form

An endpoint built by hand reads left to right: protocol, address, credential, model name. Every model made from one endpoint shares its connection pool.

```rust
use llm_provider::{Llm, OpenAi};

fn qwen(key: &str) -> Result<Llm, llm_provider::Error> {
    Ok(OpenAi::builder("http://gpu-box:8000/v1")? // protocol, address
        .credential(key)                         // credential
        .build()
        .llm("qwen3"))                           // model name
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
| `bedrock` | Claude and Kimi K3 on Amazon Bedrock with one Bedrock API key | `AWS_BEARER_TOKEN_BEDROCK=… cargo run --example bedrock` |
| `cache` | a one-hour prompt cache on Bedrock: written once, read back, `Usage` printed | `AWS_BEARER_TOKEN_BEDROCK=… cargo run --example cache` |
| `own_provider` | an own provider on `core`: Ollama's native `/api/generate` (needs a local Ollama) | `cargo run --example own_provider` |
| `session_store` | a session stored as JSON lines, restored from its events, compacted through a summarizer | `cargo run --example session_store --features agent` |
| `two_providers` | Claude plans, a local model writes | `cargo run --example two_providers --features agent` |

## Tools from outside

`llm-provider-tools` turns the tools of any MCP server, local over stdio or remote over Streamable HTTP, into `llm-provider` tools. Its install line, example and limits are in [its README](crates/llm-provider-tools/README.md).

## Manual

The manual is the crate documentation: `cargo doc --open --all-features` ([docs.rs](https://docs.rs/llm-provider)).

- `llm_provider` (crate root): the quick tour and the features.
- `provider`: the endpoints `Anthropic`, `OpenAi` and `Bedrock`, the models `Llm` and `Embedder`, protocols, credentials, typed answers and the JSON Schema subset (`Request::send_as`), timeouts (`ProviderOptions`), retry (`RetryPolicy`), size caps.
- `agent`: the turn loop, `#[tool]` and `#[tools]`, `Advisor` and `CONSULT_ADVISOR`, agents as tools (`Session::into_tool`), turn limits (`SessionBuilder`).
- `fs_tools`: the file tools (`Files`) and what the `Sandbox` covers.

## Tested with

Verified live: Claude Haiku, Sonnet, Opus and Fable 5.x on the Anthropic API, Claude on Amazon Bedrock and Kimi K3 on Bedrock, each with ask, stream, `send_json` and a tool turn: `cargo run --example verify --all-features` (your own `ANTHROPIC_API_KEY` or `AWS_BEARER_TOKEN_BEDROCK`; `--route anthropic|bedrock|kimi|ollama`, `--model <id>` select a subset).

## What the caller decides

- **Credentials:** passed in as an `AuthMethod`, never looked up; `Debug` masks them.
- **Where requests go:** each endpoint names its address. `Anthropic::new(key)` sends to `https://api.anthropic.com`, `OpenAi::new(key)` to `https://api.openai.com/v1`, `OpenAi::at(url)` to the server you name, and `Bedrock::new(key, region)` to `https://bedrock-runtime.{region}.amazonaws.com`, under `/anthropic` or `/openai/v1`.
- **Timeouts and retry:** `ProviderOptions` (30 s connect, 90 s silence) and `RetryPolicy` (4 attempts).
- **The sandbox:** file tools resolve every path inside one root; no guard against races or hard links.
- **Model output is untrusted:** a tool validates its input before acting on it.

## Security

CI runs `scripts/security.sh` on every push to `main`, every pull request and weekly: `cargo deny` (advisories, licences, bans, sources), `osv-scanner`, `gitleaks` and `semgrep`.

To report a vulnerability, use GitHub's private vulnerability reporting (the Security tab of [appdoo/llm-provider](https://github.com/appdoo/llm-provider/security)). Please do not open a public issue for it.

## License

MIT. See `LICENSE`.
