# llm-provider

## What it is

`llm-provider` is a small, pure-Rust toolbox for LLM agents. It covers the
range from one completion to a tool-using agent loop.

## Status and support

The crate is at 0.x, so a minor version may break the API. It is built for
the author's own projects and maintained while they use it. Issues and pull
requests are welcome; there is no guaranteed support.

## Protocols

- **Anthropic Messages API**: Claude, with prompt caching, message batches
  and OAuth login.
- **OpenAI Chat Completions API**: OpenAI and every server that speaks it,
  such as Ollama, vLLM, LM Studio, the llama.cpp server, `OpenRouter`, Groq and
  Mistral.

## Levels

Each level builds on the one before it. Only level 0 is on by default.

| Level | Module | Feature | Adds dependencies |
|---|---|---|---|
| 0: providers | `provider` | always on | none beyond the base set |
| 1: agent loop | `agent` | `agent` | `futures` |
| File tools | `fs_tools` | `fs-tools` (turns on `agent`) | `glob`, `regex`, `tempfile`, `walkdir` |

The base set is `reqwest` (rustls), `tokio`, `serde`, `serde_json`,
`thiserror`, `async-trait`, `secrecy` (credentials), `schemars` (typed
answers), `backon` (retry schedule)
and a few small stream and byte crates.

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["fs-tools"] }
```

## Quick start

The level 1 and file-tool examples need their features. The crate's doc
tests compile all examples with `cargo test --all-features`; without those
features, the level 1 and file-tool examples do not compile.

### Level 0: one question

```rust,no_run
use llm_provider::Anthropic;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The caller supplies the credential; the crate reads no environment variable.
    let api_key = std::env::var("ANTHROPIC_API_KEY")?;
    let claude = Anthropic::new(api_key, "claude-sonnet-4-6");
    let answer = claude.ask("What is a borrow checker?").await?;
    println!("{answer}");

    let reply = claude
        .request("What is a borrow checker?")
        .system("Answer in one sentence.")
        .max_tokens(256)
        .send()
        .await?;
    println!("{}", reply.text);
    Ok(())
}
```

Retry is on by default (4 attempts);
`Anthropic::builder(key, model).retry(RetryPolicy::none()).build()` turns it
off. `.into_request()` gives the full `CompletionRequest` for streaming, tools
or batches.

### Level 0: a typed answer

`ask_json` asks for the answer as your own type; its schema, derived with
`JsonSchema`, reaches the model as a forced tool (Anthropic) or a strict
response format (OpenAI-compatible). Checks beyond the schema stay yours.

```rust,no_run
use llm_provider::{Anthropic, JsonSchema};
use serde::Deserialize;

/// What a text is about.
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(std::fs::read_to_string("anthropic.key")?.trim(), "claude-sonnet-4-6");

    let summary: Summary = claude.ask_json("Summarize: Rust is a systems language …").await?;
    println!("{}", summary.sentences.join(" "));

    let brief = claude
        .request("Summarize: Rust is a systems language …")
        .system("Be brief.")
        .send_json::<Summary>()
        .await?;
    println!("{}", brief.key_points.join("; "));
    Ok(())
}
```

### A local model

```rust,no_run
use llm_provider::OpenAiCompatible;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ollama = OpenAiCompatible::new("http://localhost:11434/v1", "llama3.2");
    let answer = ollama.ask("What is a borrow checker?").await?;
    println!("{answer}");
    Ok(())
}
```

No key is sent unless the builder sets one:
`OpenAiCompatible::builder(url, model).credential("sk-…").build()` sends
`Authorization: Bearer sk-…`.

### Level 1: an agent turn with a tool

A `Tool` tells the model its name, description and input schema, and runs
on the model's input. `Tools` collects them; `ConversationRuntime` streams a
completion, runs the requested tools, feeds the results back and repeats
until the model stops.

```rust,no_run
# #[cfg(feature = "agent")]
use llm_provider::agent::{ConversationRuntime, Tool, ToolOutput, Tools};
# #[cfg(feature = "agent")]
use llm_provider::provider::ToolDefinition;
# #[cfg(feature = "agent")]
use llm_provider::{Anthropic, AuthMethod};
# #[cfg(feature = "agent")]
use serde_json::{json, Value};

# #[cfg(feature = "agent")]
struct Clock;

# #[cfg(feature = "agent")]
#[async_trait::async_trait]
impl Tool for Clock {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "unix_time".into(),
            description: "Current time in seconds since 1970-01-01 UTC.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: Value) -> ToolOutput {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        ToolOutput { content: secs.to_string(), is_error: false }
    }
}

# #[cfg(feature = "agent")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY")?;
    let model = Anthropic::new(AuthMethod::from(key), "claude-sonnet-4-6");

    let mut agent = ConversationRuntime::new(model, Tools::new().with(Clock));
    let turn = agent.run_turn("What is the Unix time now?").await?;
    println!("{} ({} tool calls)", turn.response, turn.tool_calls);
    Ok(())
}
# #[cfg(not(feature = "agent"))]
# fn main() {}
```

### An agent that edits files

An agent on a local OpenAI-compatible server that may read and edit files
in one directory. Streamed text goes to `on_delta`.

```rust,no_run
# #[cfg(feature = "fs-tools")]
use llm_provider::agent::{ConversationRuntime, Tools};
# #[cfg(feature = "fs-tools")]
use llm_provider::fs_tools::{EditTool, ReadTool, Sandbox};
# #[cfg(feature = "fs-tools")]
use llm_provider::OpenAiCompatible;

# #[cfg(feature = "fs-tools")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");

    let sandbox = Sandbox::new("docs")?;
    let tools = Tools::new()
        .with(ReadTool::new(sandbox.clone()))
        .with(EditTool::new(sandbox));

    let mut agent = ConversationRuntime::builder(model, tools)
        .system("You fix typos in the files you are given.")
        .max_iterations(10)
        .on_delta(|text| print!("{text}"))
        .build();

    let turn = agent.run_turn("Fix the typos in README.md.").await?;
    eprintln!("{} tool calls", turn.tool_calls);
    Ok(())
}
# #[cfg(not(feature = "fs-tools"))]
# fn main() {}
```

### File tools: read inside a sandbox

`Sandbox` confines every path a file tool receives to one root directory.
This example runs without a network.

```rust
# #[cfg(feature = "fs-tools")]
use llm_provider::agent::Tool;
# #[cfg(feature = "fs-tools")]
use llm_provider::fs_tools::{ReadTool, Sandbox};
# #[cfg(feature = "fs-tools")]
use serde_json::json;

# #[cfg(feature = "fs-tools")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join("llm-provider-readme");
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("notes.txt"), "hello sandbox\n")?;

    let read = ReadTool::new(Sandbox::new(&root)?);

    let inside = read.call(json!({ "path": "notes.txt" })).await;
    assert!(!inside.is_error);
    assert!(inside.content.contains("hello sandbox"));

    let outside = read.call(json!({ "path": "/etc/hosts" })).await;
    assert!(outside.is_error);
    Ok(())
}
# #[cfg(not(feature = "fs-tools"))]
# fn main() {}
```

## Different agents, different providers

Each `ConversationRuntime` owns its provider, so one program can run an
agent on Claude and another on a local model side by side.

```rust,no_run
# #[cfg(feature = "agent")]
use llm_provider::agent::{ConversationRuntime, Tools};
# #[cfg(feature = "agent")]
use llm_provider::{Anthropic, OpenAiCompatible};

# #[cfg(feature = "agent")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Claude, with a key the caller supplies.
    let key = std::env::var("ANTHROPIC_API_KEY")?;
    let claude = Anthropic::new(key, "claude-sonnet-4-6");

    // A local Ollama model through its OpenAI-compatible endpoint, no key.
    let ollama = OpenAiCompatible::new("http://localhost:11434/v1", "llama3.2");

    let mut planner = ConversationRuntime::builder(claude, Tools::new())
        .system("You plan. Reply with a numbered list.")
        .build();
    let mut writer = ConversationRuntime::builder(ollama, Tools::new())
        .system("You write short release notes.")
        .build();

    let plan = planner.run_turn("Plan a 0.2.0 release.").await?;
    let notes = writer.run_turn(&plan.response).await?;
    println!("{}", notes.response);
    Ok(())
}
# #[cfg(not(feature = "agent"))]
# fn main() {}
```

## What the caller decides

- **Credentials.** They are passed in as an `AuthMethod` (API key or OAuth
  bearer token) and never looked up: the crate reads no environment variable
  and no credential file. The Anthropic provider fails without a credential;
  the OpenAI-compatible provider sends no `authorization` header without one.
  `AuthMethod`'s `Debug` output masks the secret.
- **Where `base_url` points.** The OpenAI-compatible provider needs both
  `base_url` and `model`, so a local setup never falls through to a hosted
  API. The Anthropic provider defaults to `https://api.anthropic.com`.
- **Timeouts and retry.** `ProviderOptions` sets the connect timeout
  (default 30 s) and the longest silence on a response (default 90 s).
  `RetryPolicy` sets the attempts per request (default 4; 1 turns retry
  off). `ProviderConfig` carries all three for the factories; the front types
  and the factories retry by default.
- **Size caps.** An error body is read up to 64 KiB. An SSE line, an SSE
  event and the streamed input of one tool call are each capped at 1 MiB.
  `RuntimeBuilder::max_tool_output_chars` (default 100 000) bounds a tool
  result; longer output keeps its head and tail. `ReadTool` refuses a file
  over 10 MiB unless the call sets `offset` or `limit`.
- **The file-tool sandbox.** A tool built with a `Sandbox` canonicalizes
  every path and refuses one that resolves outside the root, including
  through symlinks. It does not cover the race between resolving a path and
  using it (another process can swap a path component in between), and it
  does not detect hard links, which point into the root from anywhere. Run
  the tools on a directory no other process writes to. A tool built without
  a `Sandbox` reads and writes wherever the process can.
- **Model output is untrusted.** Tool names and tool inputs come from the
  model. A `Tool` validates its input before acting on it.

## Security

CI runs `scripts/security.sh` on every push, every pull request and weekly.
It runs:

- `cargo deny`: advisories, licences, bans and sources;
- `osv-scanner`: a second advisory source;
- `gitleaks`: secrets in the history and the tree;
- `semgrep`: insecure Rust patterns and secrets in code.

To report a vulnerability, use GitHub's private vulnerability reporting once
the repository is public. Until then, report it privately to the repository
owner. Please do not open a public issue for it.

## License

MIT. See `LICENSE`.
