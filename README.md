# llm-provider

## What it is

`llm-provider` is a small, pure-Rust toolbox for LLM agents. It covers the
range from one completion to a tool-using agent loop.

## Status and support

The crate is at 0.x, so a minor version may break the API. It is built for
the author's own projects and maintained while they use it. Issues and pull
requests are welcome; there is no guaranteed support.

## Protocols

- **Anthropic Messages API**: Claude, with prompt caching, OAuth login and,
  with the `batch` feature, message batches.
- **OpenAI Chat Completions API**: OpenAI and every server that speaks it,
  such as Ollama, vLLM, LM Studio, the llama.cpp server, `OpenRouter`, Groq and
  Mistral.

## Levels

Each level builds on the one before it. Only level 0 is on by default.

| Level | Module | Feature | Adds dependencies |
|---|---|---|---|
| 0: providers | `provider` | always on | none beyond the base set |
| Derived answer schemas | `ResponseSchema::of`, `Request::send_json` | `schema` | `schemars` |
| 1: agent loop | `agent` | `agent` (turns on `schema`) | `futures`, `llm-provider-macros` (`#[tool]`, `#[tools]`) |
| File tools | `fs_tools` | `fs-tools` (turns on `agent`) | `glob`, `regex`, `tempfile`, `walkdir` |
| Message batches | `BatchProvider` | `batch` | none |

The base set is `reqwest` (rustls), `tokio`, `serde`, `serde_json`,
`thiserror`, `async-trait`, `secrecy` (credentials), `backon` (retry
schedule) and a few small stream and byte crates.

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["fs-tools"] }
```

## Quick start

The `schema`, level 1 and file-tool examples need their features. The
crate's doc tests compile all examples with `cargo test --all-features`;
without those features, those examples do not compile.

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
off. `.into_request()` gives the full `CompletionRequest` for streaming or
tools.

### Level 0: a typed answer

`send_as` asks for the answer as your own type, held to a `ResponseSchema`:
a name and a JSON schema. The schema reaches the model as a forced tool
(Anthropic) or a JSON-schema response format (OpenAI-compatible). Checks
beyond the schema stay yours.

```rust,no_run
use llm_provider::{Anthropic, ResponseSchema};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Summary {
    sentences: Vec<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(std::fs::read_to_string("anthropic.key")?.trim(), "claude-sonnet-4-6");

    let schema = ResponseSchema::new(
        "Summary",
        json!({
            "type": "object",
            "properties": { "sentences": { "type": "array", "items": { "type": "string" } } },
            "required": ["sentences"]
        }),
    );
    let summary: Summary = claude
        .request("Summarize: Rust is a systems language …")
        .system("Two to four sentences.")
        .send_as(schema)
        .await?;
    println!("{}", summary.sentences.join(" "));
    Ok(())
}
```

With the `schema` feature, `send_json` derives the schema from the type
instead; its doc comments become the descriptions the model reads. The
`Deserialize` and `JsonSchema` derives expand to `serde` and `schemars`
paths, so a crate that derives them adds both itself:

```toml
[dependencies]
llm-provider = { git = "https://github.com/appdoo/llm-provider", features = ["schema"] }
serde = { version = "1", features = ["derive"] }
schemars = "1"
```

```rust,no_run
# #[cfg(feature = "schema")]
use llm_provider::{Anthropic, JsonSchema};
# #[cfg(feature = "schema")]
use serde::Deserialize;

/// What a text is about.
# #[cfg(feature = "schema")]
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

# #[cfg(feature = "schema")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(std::fs::read_to_string("anthropic.key")?.trim(), "claude-sonnet-4-6");

    let summary = claude
        .request("Summarize: Rust is a systems language …")
        .system("Be brief.")
        .send_json::<Summary>()
        .await?;
    println!("{}", summary.key_points.join("; "));
    Ok(())
}
# #[cfg(not(feature = "schema"))]
# fn main() {}
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

### Level 1: an agent turn with tools

A tool is an `async fn` marked `#[tool]`: its doc comment is the description
the model reads, its one argument (a struct deriving `Deserialize` and
`JsonSchema`) the input schema, and its `Result` the output (`Err` marks the
call as failed). The macro adds a unit struct named after the function in
CamelCase (`unix_time` → `UnixTime`); `Tools` collects those; `ConversationRuntime` streams a
completion, runs the requested tools, feeds the results back and repeats
until the model stops.

```rust,no_run
# #[cfg(feature = "agent")]
use std::time::{SystemTime, UNIX_EPOCH};

# #[cfg(feature = "agent")]
use llm_provider::agent::{tool, ConversationRuntime, Tools};
# #[cfg(feature = "agent")]
use llm_provider::{Anthropic, JsonSchema};
# #[cfg(feature = "agent")]
use serde::Deserialize;

/// What a text is about.
# #[cfg(feature = "agent")]
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

/// Current time in seconds since 1970-01-01 UTC.
# #[cfg(feature = "agent")]
#[tool]
async fn unix_time() -> Result<String, std::io::Error> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(now.as_secs().to_string())
}

/// Which lines of a text to read.
# #[cfg(feature = "agent")]
#[derive(Deserialize, JsonSchema)]
struct LineRange {
    /// The text.
    text: String,
    /// The first line, counting from 0.
    from: usize,
    /// How many lines; all when left out.
    count: Option<usize>,
}

/// Returns some lines of a text.
# #[cfg(feature = "agent")]
#[tool]
async fn lines(range: LineRange) -> Result<String, std::convert::Infallible> {
    let lines = range.text.lines().skip(range.from).take(range.count.unwrap_or(usize::MAX));
    Ok(lines.collect::<Vec<_>>().join("\n"))
}

# #[cfg(feature = "agent")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let claude = Anthropic::new(std::fs::read_to_string("anthropic.key")?.trim(), "claude-sonnet-4-6");

    let summary = claude
        .request("Summarize: Rust is a systems language …")
        .send_json::<Summary>()
        .await?;
    println!("{}", summary.sentences.join(" "));

    let mut agent = ConversationRuntime::new(claude, Tools::new().with(UnixTime).with(Lines));
    let turn = agent.run_turn("What is the Unix time now?").await?;
    println!("{}", turn.response);
    Ok(())
}
# #[cfg(not(feature = "agent"))]
# fn main() {}
```

`#[tool(name = "now")]` gives the tool another name; the struct keeps its
CamelCase name.

### Tools that share state

Tools that share state are methods of one struct, marked `#[tool]` in a
`#[tools]` impl block. `Tools::with_all` moves the struct into one `Arc` that
every tool of the block shares; `Tools::only` keeps the named tools. The file
tools are such a block, `Files`.

```rust,no_run
# #[cfg(feature = "fs-tools")]
use llm_provider::agent::{tools, Tools};
# #[cfg(feature = "fs-tools")]
use llm_provider::fs_tools::{Files, Sandbox};
# #[cfg(feature = "fs-tools")]
use llm_provider::JsonSchema;
# #[cfg(feature = "fs-tools")]
use serde::Deserialize;

/// Pages from the web, through one shared client.
# #[cfg(feature = "fs-tools")]
struct Web {
    http: reqwest::Client,
}

/// Which page to fetch.
# #[cfg(feature = "fs-tools")]
#[derive(Deserialize, JsonSchema)]
struct Page {
    /// The page's URL.
    url: String,
}

# #[cfg(feature = "fs-tools")]
#[tools]
impl Web {
    /// Fetches a web page as text.
    #[tool]
    async fn fetch(&self, page: Page) -> Result<String, reqwest::Error> {
        self.http.get(&page.url).send().await?.text().await
    }
}

# #[cfg(feature = "fs-tools")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new("docs")?))
        .only(["read", "edit"])
        .with_all(Web { http: reqwest::Client::new() });
#   let _ = tools;
    Ok(())
}
# #[cfg(not(feature = "fs-tools"))]
# fn main() {}
```

### A tool by hand

A tool the macros do not cover, such as one whose name or schema is decided
at run time, implements `Tool` itself. Its `call` receives the calling
conversation as a `Transcript`, which it may read or ignore.

```rust,no_run
# #[cfg(feature = "agent")]
use llm_provider::agent::{Tool, ToolOutput, Tools, Transcript};
# #[cfg(feature = "agent")]
use llm_provider::provider::ToolDefinition;
# #[cfg(feature = "agent")]
use serde_json::{json, Value};

/// Answers every call with the same text, under a name given at run time.
# #[cfg(feature = "agent")]
struct Echo {
    name: String,
    reply: String,
}

# #[cfg(feature = "agent")]
#[async_trait::async_trait]
impl Tool for Echo {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: format!("Answers {}.", self.reply),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: Value, _transcript: &Transcript<'_>) -> ToolOutput {
        ToolOutput { content: self.reply.clone(), is_error: false }
    }
}

# #[cfg(feature = "agent")]
fn main() {
    let name = std::env::args().nth(1).unwrap_or_else(|| "echo".into());
    let tools = Tools::new().with(Echo { name, reply: "pong".into() });
#   let _ = tools;
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
use llm_provider::fs_tools::{Files, Sandbox};
# #[cfg(feature = "fs-tools")]
use llm_provider::OpenAiCompatible;

# #[cfg(feature = "fs-tools")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = OpenAiCompatible::new("http://localhost:11434/v1", "qwen3");

    let tools = Tools::new()
        .with_all(Files::new(Sandbox::new("docs")?))
        .only(["read", "edit"]);

    let mut agent = ConversationRuntime::builder(model, tools)
        .system("You fix typos in the files you are given.")
        .max_iterations(10)
        .on_delta(|text| print!("{text}"))
        .build();

    let turn = agent.run_turn("Fix the typos in README.md.").await?;
    eprintln!("{} tool calls", turn.tool_calls.len());
    Ok(())
}
# #[cfg(not(feature = "fs-tools"))]
# fn main() {}
```

### Agents that advise and ask each other

Two pieces make advisors, critics and delegating agents a few lines of
caller code. Every tool call receives the calling agent's conversation as a
`Transcript`; `Transcript::render` turns it into text another model reads
as data in a user message. And `ConversationRuntime::into_tool` makes an
agent a tool another agent calls with `{"prompt": "…"}`. A turn's
`tool_calls` records every call with its input and output.

An advisor pays off: Anthropic reports Sonnet with an Opus advisor 2.7
points higher on SWE-bench Multilingual at 11.9 % lower cost per task, and
Haiku with an Opus advisor at 41.2 % on `BrowseComp` against 19.7 % alone. A
model criticising its own work does not improve it; a second role with
another view does (Huang et al. 2024; Du et al. 2023; Liang et al. 2023).

Opus works, Fable advises. `CONSULT_ADVISOR` tells the agent when to call;
the caller puts it in the system prompt.

```rust,no_run
# #[cfg(feature = "fs-tools")]
# mod example {
use llm_provider::agent::{Advisor, ConversationRuntime, Tools, CONSULT_ADVISOR};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::Anthropic;

# const CODER: &str = "You fix code.";
# async fn run(key: String) -> Result<(), Box<dyn std::error::Error>> {
let opus = Anthropic::new(key.as_str(), "claude-opus-4-1");
let advisor = Advisor::new(Anthropic::new(key.as_str(), "claude-fable-5-1")).max_uses(3);
let tools = Tools::new().with_all(Files::new(Sandbox::new("src")?)).with(advisor);

let mut coder = ConversationRuntime::builder(opus.clone(), tools)
    .system(format!("{CODER}\n\n{CONSULT_ADVISOR}"))
    .build();
coder.run_turn("Make the failing test in parser.rs pass.").await?;
# Ok(())
# }
# }
# fn main() {}
```

A writer with two critics; their disagreement stays visible:

```rust,no_run
# #[cfg(feature = "agent")]
# mod example {
use std::fs::read_to_string;

use llm_provider::agent::{Advisor, ConversationRuntime, Tools};
use llm_provider::Anthropic;

# async fn run(opus: Anthropic) -> Result<(), Box<dyn std::error::Error>> {
let eschbach = Advisor::new(opus.clone())
    .name("eschbach")
    .description("Critic: plot logic, plausibility, what to cut.")
    .system(read_to_string("agents/eschbach.md")?);
let editor = Advisor::new(opus.clone())
    .name("editor")
    .description("Critic: rhythm, clarity, repetition.")
    .system(read_to_string("agents/editor.md")?);

let mut writer = ConversationRuntime::builder(opus.clone(), Tools::new().with(eschbach).with(editor))
    .system(read_to_string("agents/schaetzing.md")?)
    .build();

let turn = writer.run_turn("Revise chapter 1, scene 3.").await?;
for call in &turn.tool_calls {
    println!("--- {} ---\n{}", call.name, call.output.content);
}
# Ok(())
# }
# }
# fn main() {}
```

A lead that delegates to two agents and asks an advisor (`Web` is a
`#[tools]` block like the one above):

```rust,no_run
# #[cfg(feature = "fs-tools")]
# mod example {
use llm_provider::agent::{tools, Advisor, ConversationRuntime, Tools};
use llm_provider::fs_tools::{Files, Sandbox};
use llm_provider::Anthropic;
# struct Web;
# #[tools]
# impl Web {
#     /// Searches the web.
#     #[tool]
#     async fn search(&self) -> Result<String, std::convert::Infallible> { Ok(String::new()) }
# }
# impl Web { fn new() -> Self { Web } }

# async fn run(haiku: Anthropic, sonnet: Anthropic, opus: Anthropic, fable: Anthropic) -> Result<(), Box<dyn std::error::Error>> {
# let sandbox = Sandbox::new("src")?;
let researcher = ConversationRuntime::builder(haiku, Tools::new().with_all(Web::new()))
    .system("You answer one research question with sources.")
    .build()
    .into_tool("researcher", "Looks something up on the web. Give it one question.");
let coder = ConversationRuntime::builder(sonnet, Tools::new().with_all(Files::new(sandbox)))
    .build()
    .into_tool("coder", "Edits files in src/. Give it one task.");

let mut lead = ConversationRuntime::builder(opus, Tools::new().with(researcher).with(coder).with(Advisor::new(fable)))
    .system("You lead; delegate, then verify.")
    .build();
let turn = lead.run_turn("Add retry to the HTTP client.").await?;
# let _ = turn;
# Ok(())
# }
# }
# fn main() {}
```

Calls to one agent run one at a time; calls to different agents run in
parallel. Agents in two tasks talk through a channel, with no new type:

```rust,no_run
# #[cfg(feature = "agent")]
# mod example {
use std::convert::Infallible;

use llm_provider::agent::tools;
use llm_provider::JsonSchema;
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

/// A mailbox shared by two agents in two tasks.
struct Mailbox {
    tx: mpsc::Sender<String>,
    rx: Mutex<mpsc::Receiver<String>>,
}

/// A message for the other agent.
#[derive(Deserialize, JsonSchema)]
struct Note {
    /// The message.
    text: String,
}

#[tools]
impl Mailbox {
    /// Sends a message to the other agent.
    #[tool]
    async fn send(&self, note: Note) -> Result<String, mpsc::error::SendError<String>> {
        self.tx.send(note.text).await.map(|()| "sent".into())
    }

    /// Waits for the other agent's next message.
    #[tool]
    async fn receive(&self) -> Result<String, Infallible> {
        Ok(self.rx.lock().await.recv().await.unwrap_or_default())
    }
}
// Two Mailbox values with crossed channels, one per runtime, each runtime in its own task.
# }
# fn main() {}
```

### File tools: read inside a sandbox

`Sandbox` confines every path a file tool receives to one root directory.
This example runs without a network.

```rust
# #[cfg(feature = "fs-tools")]
use llm_provider::agent::{Toolbox, Tools, Transcript};
# #[cfg(feature = "fs-tools")]
use llm_provider::fs_tools::{Files, Sandbox};
# #[cfg(feature = "fs-tools")]
use serde_json::json;

# #[cfg(feature = "fs-tools")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join("llm-provider-readme");
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("notes.txt"), "hello sandbox\n")?;

    let tools = Tools::new().with_all(Files::new(Sandbox::new(&root)?));

    let inside = tools.call("read", json!({ "path": "notes.txt" }), &Transcript::default()).await;
    assert!(!inside.is_error);
    assert!(inside.content.contains("hello sandbox"));

    let outside = tools.call("read", json!({ "path": "/etc/hosts" }), &Transcript::default()).await;
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
  off). The front types retry by default. For a provider named in a
  configuration file, the optional `config` feature adds `ProviderConfig`
  and `create_provider`.
- **Size caps.** An error body is read up to 64 KiB. An SSE line, an SSE
  event and the streamed input of one tool call are each capped at 1 MiB.
  `RuntimeBuilder::max_tool_output_chars` (default 100 000) bounds a tool
  result; longer output keeps its head and tail. The `read` file tool refuses a file
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
