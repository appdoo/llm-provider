# llm-provider

A small, pure-Rust toolbox for LLM agents, from one completion to a tool-using agent loop: the Anthropic Messages API and every OpenAI-compatible server, typed answers, an agent loop with `#[tool]` and sandboxed file tools.

The crate is at 0.x, so a minor version may break the API.

## Install

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["agent"] }
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

The features, the building blocks and the examples are in the [repository README](https://github.com/appdoo/llm-provider#readme); the manual is the crate documentation.

## License

MIT. See `LICENSE`.
