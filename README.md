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
| File tools | `fs_tools` | `fs-tools` (turns on `agent`) | `glob`, `regex`, `uuid` |

The base set is `reqwest` (rustls), `tokio`, `serde`, `serde_json`,
`thiserror`, `async-trait`, `secrecy` (credentials), `backon` (retry schedule)
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

A `ToolExecutor` lists the tools the model may call and runs them by name.
`ConversationRuntime` streams a completion, runs the requested tools, feeds
the results back and repeats until the model stops.

```rust,no_run
# #[cfg(feature = "agent")]
use llm_provider::agent::{ConversationRuntime, ToolExecResult, ToolExecutor};
# #[cfg(feature = "agent")]
use llm_provider::provider::{create_provider, AuthMethod, ProviderConfig, ToolDefinition};
# #[cfg(feature = "agent")]
use serde_json::{json, Value};

# #[cfg(feature = "agent")]
struct Clock;

# #[cfg(feature = "agent")]
#[async_trait::async_trait]
impl ToolExecutor for Clock {
    async fn execute(&self, tool_name: &str, _input: Value) -> ToolExecResult {
        match tool_name {
            "unix_time" => {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                ToolExecResult { output: secs.to_string(), is_error: false }
            }
            other => ToolExecResult { output: format!("unknown tool {other}"), is_error: true },
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "unix_time".into(),
            description: "Current time in seconds since 1970-01-01 UTC.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }]
    }
}

# #[cfg(feature = "agent")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY")?;
    let provider = create_provider(&ProviderConfig::default(), Some(AuthMethod::from(key)))?;

    let mut runtime = ConversationRuntime::new(provider.as_ref(), Clock, None);
    let summary = runtime.run_turn("What is the Unix time now?", None).await?;
    println!("{} ({} tool calls)", summary.response, summary.tool_calls);
    Ok(())
}
# #[cfg(not(feature = "agent"))]
# fn main() {}
```

### File tools: read inside a sandbox

`Sandbox` confines every path a file tool receives to one root directory.
This example runs without a network.

```rust
# #[cfg(feature = "fs-tools")]
use llm_provider::agent::tool::ToolExecutor;
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

    let read = ReadTool::with_sandbox(Sandbox::new(&root)?);

    let inside = read.execute(json!({ "path": "notes.txt" })).await;
    assert!(!inside.is_error);
    assert!(inside.content.contains("hello sandbox"));

    let outside = read.execute(json!({ "path": "/etc/hosts" })).await;
    assert!(outside.is_error);
    Ok(())
}
# #[cfg(not(feature = "fs-tools"))]
# fn main() {}
```

## Different agents, different providers

Each `ConversationRuntime` borrows its own provider, so one program can run
an agent on Claude and another on a local model side by side.

```rust,no_run
# #[cfg(feature = "agent")]
use llm_provider::agent::{ConversationRuntime, ToolExecResult, ToolExecutor};
# #[cfg(feature = "agent")]
use llm_provider::provider::{
    create_anthropic, create_provider, AuthMethod, ProviderConfig, ProviderKind, ToolDefinition,
};
# #[cfg(feature = "agent")]
use serde_json::Value;

# #[cfg(feature = "agent")]
/// An executor that offers no tools.
struct NoTools;

# #[cfg(feature = "agent")]
#[async_trait::async_trait]
impl ToolExecutor for NoTools {
    async fn execute(&self, tool_name: &str, _input: Value) -> ToolExecResult {
        ToolExecResult { output: format!("unknown tool {tool_name}"), is_error: true }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }
}

# #[cfg(feature = "agent")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Claude, with a key the caller supplies.
    let key = std::env::var("ANTHROPIC_API_KEY")?;
    let claude = create_anthropic(&ProviderConfig::default(), AuthMethod::from(key));

    // A local Ollama model through its OpenAI-compatible endpoint, no key.
    let ollama = create_provider(
        &ProviderConfig::default()
            .provider(ProviderKind::OpenAi)
            .model("llama3.2")
            .base_url("http://localhost:11434/v1"),
        None,
    )?;

    let mut planner = ConversationRuntime::new(
        &claude,
        NoTools,
        Some("You plan. Reply with a numbered list.".into()),
    );
    let mut writer = ConversationRuntime::new(
        ollama.as_ref(),
        NoTools,
        Some("You write short release notes.".into()),
    );

    let plan = planner.run_turn("Plan a 0.2.0 release.", None).await?;
    let notes = writer.run_turn(&plan.response, None).await?;
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
  `LoopDefaults::max_tool_output_chars` (default 100 000) bounds a tool
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
  model. A `ToolExecutor` validates them before acting on them.

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
