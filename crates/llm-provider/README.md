# llm-provider

A Rust API for LLMs.

Anthropic, Amazon Bedrock, OpenAI and OpenAI-compatible servers (Ollama, vLLM): ask, stream, typed answers, tools and agents.

- Prompt caching, on by default in the agent loop; `Usage::cost` gives the cost of a call or a turn.
- Typed answers (`send_json`), tools from a plain function (`#[tool]`), MCP servers as tools.
- A bad tool call or a hung tool becomes an error result for the model; a failed turn returns its work.
- `agent`, `schema`, `fs-tools` and `batch` are opt-in features.

0.x: a minor version may break the API; every break is in the [CHANGELOG](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/CHANGELOG.md) with a before/after table.

## Install

```toml
[dependencies]
llm-provider = { version = "0.3", features = ["agent"] }
```

## Ask

```rust
use llm_provider::Anthropic;

async fn ask(key: &str) -> Result<String, llm_provider::Error> {
    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    claude.ask("What is a borrow checker?").await
}
```

## Stream

The answer as it is written. `futures::StreamExt` is your own import: `futures = "0.3"` in your dependencies.

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

## A typed answer

With `schema`, the answer parses into your type and its doc comments are what the model reads. The derives need `serde` and `schemars` in your own dependencies.

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

## A tool

With `agent`, `#[tool]` turns an async function into a tool and the agent loop calls it when the model asks.

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

## An agent that prints its cost

```rust,no_run
use llm_provider::{agent::{tool, Session, Tools}, Anthropic, Prices};

/// The current weather in Graz.
#[tool]
async fn weather() -> Result<String, std::convert::Infallible> { Ok("Rain, 9 C".into()) }

async fn run(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = Session::new(Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5), Tools::new().with(Weather));
    let turn = agent.run_turn("Do I need an umbrella?").await?;
    println!("{}\n${:.4}", turn.response, turn.usage.cost(&Prices::per_million(1.0, 5.0)));
    Ok(())
}
```

## Next

- [Caching and cost](https://docs.rs/llm-provider/latest/llm_provider/struct.Prices.html): `Prices` and `Usage::cost`
- [Your own provider](https://docs.rs/llm-provider/latest/llm_provider/core/index.html): the `core` module
- [MCP servers as tools](https://github.com/appdoo/llm-provider/tree/main/crates/llm-provider-tools)
- [Examples](https://github.com/appdoo/llm-provider/blob/tree/main/crates/llm-provider/examples)
- [Repository README](https://github.com/appdoo/llm-provider#readme) and [docs.rs](https://docs.rs/llm-provider)

## License

MIT. See [LICENSE](https://github.com/appdoo/llm-provider/blob/main/LICENSE).
