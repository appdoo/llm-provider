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

The features, the building blocks and the examples are in the [repository README](https://github.com/appdoo/llm-provider#readme); the manual is the crate documentation.

## License

MIT. See `LICENSE`.
