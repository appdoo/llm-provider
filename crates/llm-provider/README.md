# llm-provider

A small, pure-Rust toolbox for LLM agents, from one completion to a tool-using agent loop: the Anthropic Messages API and every OpenAI-compatible server, typed answers, an agent loop with `#[tool]` and sandboxed file tools.

The crate is at 0.x, so a minor version may break the API.

## Install

```toml
[dependencies]
llm-provider = { version = "0.3", features = ["agent"] }
```

## One question

```rust,no_run
use llm_provider::Anthropic;

async fn ask(key: &str) -> Result<String, llm_provider::Error> {
    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    claude.ask("What is a borrow checker?").await
}
```

## Build your own provider

Our providers send through a public core, and yours can too. A `Transport` knows the address, your headers (`bearer`, `header`) and the retry before the first content event; `send_body` POSTs a JSON body of your making and hands the reply to a decoder. `decode::lines` frames server-sent events and newline-delimited JSON, so a provider is the body it sends and the line it reads. The program below talks to Ollama's native `/api/generate`:

```rust,no_run
use llm_provider::core::{decode, Transport};
use llm_provider::provider::{
    CompletionRequest, EventStream, LlmProvider, ProviderInfo, StreamEvent,
};
use llm_provider::Llm;
use serde_json::json;

struct Ollama {
    transport: Transport,
    model: String,
}

impl LlmProvider for Ollama {
    fn stream(&self, request: CompletionRequest) -> EventStream {
        let prompt = request.last_user_text().unwrap_or_default();
        let body = json!({ "model": self.model, "prompt": prompt, "stream": true });
        self.transport.send_body("/api/generate", body, |bytes| {
            decode::lines(bytes, |line| {
                let v: serde_json::Value = serde_json::from_str(line)?;
                Ok(v["response"].as_str().map(StreamEvent::text))
            })
        })
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("ollama", &self.model)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transport = Transport::at("http://localhost:11434")?; // .bearer(key) when needed
    let llm = Llm::from_provider(Ollama {
        transport,
        model: "qwen3".into(),
    });
    println!("{}", llm.ask("Wie viele Ziegen?").await?);
    Ok(())
}
```

## Caching and sampling

A long prefix that stays the same between calls (a handbook in the system prompt, the tool list, the conversation so far) can be cached: `cache` marks the last tool, the system prompt and the last message, and the provider reads them back at a tenth of the input price. It works at Anthropic and on Amazon Bedrock; OpenAI caches on its own and ignores the marks. The agent loop caches by default for five minutes (`ConversationRuntime::builder(..).cache(CacheTtl::OneHour)` or `.no_cache()`). A cache write costs 1.25 times the input price (2 times for one hour) and a read 0.1 times, so the cache pays when the next call comes within its lifetime: right for a tool loop, wrong for a chat with minutes between turns, which should use `CacheTtl::OneHour` or `.no_cache()`. Below the model's minimum size the provider caches nothing and says nothing, so check `Usage` ([`examples/cache.rs`](crates/llm-provider/examples/cache.rs)).

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
use llm_provider::agent::{ConversationRuntime, RuntimeError, Tools};
use tokio_util::sync::CancellationToken;

async fn plan(agent: &mut ConversationRuntime<Tools>, token: &CancellationToken) {
    match agent.run_turn_cancellable("Plan the refactor.", token).await {
        Err(RuntimeError::Interrupted { partial_text, tool_calls, .. }) => {
            println!("cut after {} tool calls: {partial_text}", tool_calls.len());
        }
        other => println!("{other:?}"),
    }
}
```

The features, the building blocks and the examples are in the [repository README](https://github.com/appdoo/llm-provider#readme); the manual is the crate documentation.

## License

MIT. See `LICENSE`.
