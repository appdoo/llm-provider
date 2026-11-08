# llm-provider

A small, pure-Rust toolbox for LLM agents, from one completion to a tool-using agent loop.

- **A small front API.** `Anthropic::new(key, model).ask("…")` answers one question.
- **Two protocols.** The Anthropic Messages API in depth (prompt caching, OAuth, structured outputs, batches), and every server that speaks the OpenAI Chat Completions API: OpenAI, Ollama, vLLM, LM Studio, llama.cpp, `OpenRouter`, Groq, Mistral.
- **Agents from a few pieces that compose.** Tools, agents as tools, and advisors.

The crate is at 0.x, so a minor version may break the API. It is built for the author's own projects; issues and pull requests are welcome, with no guaranteed support.

## Install

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["agent"] }
```

| Feature | Adds |
|---|---|
| default | the providers: `Anthropic`, `OpenAiCompatible`, `ask`, `request`, `send_as` |
| `schema` | `send_json` and `ResponseSchema::of`: the answer schema derived from your type |
| `agent` | the agent loop `ConversationRuntime`, `#[tool]`, `#[tools]`, `Advisor` (turns on `schema`) |
| `fs-tools` | the sandboxed file tools `Files`: read, write, edit, grep, glob (turns on `agent`) |
| `batch` | Anthropic message batches |
| `config` | `ProviderConfig` and `create_provider` for providers named in a config file |

## Quick start

One question:

```rust
let claude = Anthropic::new(api_key, "claude-haiku-5-5");
let answer = claude.ask("What is a borrow checker?").await?;
```

A typed answer (`schema`); the doc comments are what the model reads:

```rust
/// What a text is about.
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

let summary = claude.request(text).system("Be brief.").send_json::<Summary>().await?;
```

An agent with a tool (`agent`):

```rust
/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string())
}

let mut agent = ConversationRuntime::new(claude, Tools::new().with(UnixTime));
let turn = agent.run_turn("What is the Unix time now?").await?;
println!("{}", turn.response);
```

An agent with an advisor (`fs-tools`): Opus works, Fable reads the whole conversation when Opus asks:

```rust
let opus = Anthropic::new(key.as_str(), "claude-opus-5-5");
let advisor = Advisor::new(Anthropic::new(key.as_str(), "claude-fable-5-1")).max_uses(2);
let tools = Tools::new().with_all(Files::new(Sandbox::new(dir)?)).with(advisor);

let mut coder = ConversationRuntime::builder(opus, tools)
    .system(format!("You fix code.\n\n{CONSULT_ADVISOR}"))
    .on_delta(|text| print!("{text}"))
    .build();
let turn = coder.run_turn("Make stats.py meet TASK.md.").await?;
```

## Examples

Each file in `examples/` is a complete program. The Claude examples read the key from `ANTHROPIC_API_KEY` (the example does; the crate never reads an environment variable) and exit 2 with a usage line without it. The local-model examples take `[BASE_URL] [MODEL]`, default `http://localhost:11434/v1` and `llama3.2`.

| Example | What it shows | Run |
|---|---|---|
| `ask` | one question, then a request with a system prompt and a token limit | `cargo run --example ask` |
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

## Manual

The manual is the crate documentation: `cargo doc --open --all-features` (docs.rs once published).

- `llm_provider` (crate root): the levels and their features.
- `provider`: protocols, credentials, typed answers and the JSON Schema subset (`Request::send_as`), timeouts (`ProviderOptions`), retry (`RetryPolicy`), size caps.
- `agent`: the turn loop, `#[tool]` and `#[tools]`, `Advisor` and `CONSULT_ADVISOR`, agents as tools (`ConversationRuntime::into_tool`), turn limits (`RuntimeBuilder`).
- `fs_tools`: the file tools (`Files`) and what the `Sandbox` covers.

## What the caller decides

- **Credentials:** passed in as an `AuthMethod`, never looked up; `Debug` masks them.
- **Where requests go:** `OpenAiCompatible` needs `base_url` and `model`; `Anthropic` defaults to `https://api.anthropic.com`.
- **Timeouts and retry:** `ProviderOptions` (30 s connect, 90 s silence) and `RetryPolicy` (4 attempts).
- **The sandbox:** file tools resolve every path inside one root; no guard against races or hard links.
- **Model output is untrusted:** a tool validates its input before acting on it.

## Security

CI runs `scripts/security.sh` on every push, every pull request and weekly: `cargo deny` (advisories, licences, bans, sources), `osv-scanner`, `gitleaks` and `semgrep`.

To report a vulnerability, use GitHub's private vulnerability reporting once the repository is public. Until then, report it privately to the repository owner. Please do not open a public issue for it.

## License

MIT. See `LICENSE`.
