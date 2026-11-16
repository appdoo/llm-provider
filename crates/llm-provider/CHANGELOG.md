# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [0.2.1](https://github.com/appdoo/llm-provider/compare/llm-provider-v0.2.0...llm-provider-v0.2.1) - 2026-11-16

### Other

- tools at 0.2.0, workspace dependencies for llm-provider, rmcp, tokio-stream, trybuild
- tools joins the llm-provider version group; release only from the release PR; changelog fixes

### Security

- scan every crate with the full rule set; mark the five example lines that read their command line

## [0.2.0] - 2026-11-13

### Migration

A model is now reached as endpoint, then model: an endpoint (`Anthropic`, `OpenAi`, or either from `Bedrock`) holds the address, the credential and the protocol, and hands out models (`Llm`, `Embedder`) and the batch mode (`Batches`). The old names are removed, with no alias.

| Before | After |
|---|---|
| `Anthropic::new(key, m)` | `Anthropic::new(key).llm(m)` |
| `Anthropic::bedrock(key, region, m)` | `Bedrock::new(key, region).anthropic().llm(m)` |
| `Anthropic::builder(key, m).bedrock(region).build()` | `Bedrock::builder(key, region).build().anthropic().llm(m)` |
| `Anthropic::builder(key, m).base_url(u).build()` | `Anthropic::builder(key).base_url(u).build().llm(m)` |
| `OpenAiCompatible::new(url, m)` | `OpenAi::at(url).llm(m)` |
| `OpenAiCompatible::builder(url, m).credential(c).build()` | `OpenAi::builder(url).credential(c).build().llm(m)` |
| `config::create_provider(..) -> Box<dyn LlmProvider>` | `ModelBuilder<Llm>::build() -> Result<Llm, Error>`: `config.credential(auth).build()` |
| `config::create_anthropic(..) -> Anthropic` | removed (its reason was batches); `Anthropic::new(key).batches(m)` |
| `impl BatchProvider for Anthropic` | `Anthropic::new(key).batches(m)` |
| `ToolDefinition { name, description, input_schema }` | `ToolDefinition::new(name, description, input_schema)` |
| `ToolOutput { content, is_error: false }` | `ToolOutput::ok(content)` |
| `ToolOutput { content, is_error: true }` | `ToolOutput::error(content)` |
| `ToolUseCall { id, name, input }` | `ToolUseCall::new(id, name, input)` |
| `ProviderInfo { name, model }` | `ProviderInfo::new(name, model)` |
| `CompletionRequest { messages, .. }` | `CompletionRequest::new(messages)` with `system`, `model`, `max_tokens`, `tools`, `cache_breakpoints` and `response_schema` |
| `Usage { input_tokens, .. }` | `Usage::default()` with `input_tokens`, `output_tokens`, `cache_creation_input_tokens` and `cache_read_input_tokens` |
| `CacheBreakpoint { position, ttl }` | `CacheBreakpoint::new(position, ttl)` |
| `Message { role, content }` | `Message::new(role, content)` |
| `Transcript { system, messages, .. }` | `Transcript::default()`, then assign `system` and `messages` |

- Models named by values replace the configuration-file factories: `Llm::builder(model)` and `Embedder::builder(model)` return a `ModelBuilder<T>` with `protocol`, `endpoint`, `bedrock(region)`, `api_key`, `oauth_token`, `credential`, `max_attempts`, `connect_timeout_secs`, `stream_idle_timeout_secs` and `build()`; it reads from a configuration file through `serde` and never holds the credential there. Migration:
  - `ProviderConfig` → `ModelBuilder<Llm>`; the file keys `protocol`, `endpoint`, `region` and `model` replace `default` and `base_url`, which still read.
  - `ProviderKind` → `Protocol` (`Claude` → `Anthropic`, `OpenAi` stays); `Protocol` also parses from a string, any case.
  - `create_provider(&config, Some(auth))` → `config.credential(auth).build()`; `create_anthropic` is gone.
  - `ConfigError::MissingBaseUrl` → `ConfigError::MissingEndpoint`; `ConfigError::UnknownProtocol` is new.
  - The `config` feature is gone: the `config` module, `ConfigError` and `Error::Config` are always present, and `ModelBuilder` and `Protocol` are re-exported at the crate root.
- The endpoint builders are one generic type, `EndpointBuilder<E>`: `AnthropicBuilder`, `OpenAiBuilder` and `BedrockBuilder` are `pub type` names of `EndpointBuilder<Anthropic>`, `EndpointBuilder<OpenAi>` and `EndpointBuilder<Bedrock>`. Code that names the three types compiles unchanged; `timeouts`, `http_client`, `retry` and `build` are shared, `base_url` stays on the Anthropic and Bedrock builders and `credential` on the OpenAI one. A builder's `Debug` output now names `EndpointBuilder`.
- `ConversationRuntime::new`, `ConversationRuntime::builder` and `Advisor::new` take an `Llm`: `ConversationRuntime<P, T>` is `ConversationRuntime<T>`. A custom `LlmProvider` enters through `Llm::from_provider(p)`, whose model is `p.info().model`.
- `ask` and `request` are methods of the model, not of the endpoint: `Anthropic::new(key).llm(m).ask(..)`.
- `Advisor` is no longer `UnwindSafe` or `RefUnwindSafe`: it holds an `Llm`, whose protocol is a shared trait object.
- Model ids are endpoint-scoped: a `Bedrock::*` id is valid on a Bedrock endpoint, an `Anthropic::CLAUDE_*` id on the Anthropic API.
- The errors say "endpoint" and "model", not "provider":
  - `ConfigError::MissingCredential`, `MissingEndpoint` and `MissingModel`: the field `provider` → `endpoint`. Their messages read `the anthropic endpoint needs a credential`, `the openai endpoint needs an address` and `the openai endpoint needs a model` (before: `anthropic provider needs a credential`, `openai provider needs an endpoint`, `openai provider needs model`).
  - `RuntimeError::Provider` → `RuntimeError::Model`; its message reads `model error: …` (before: `Provider error: …`).

### Behaviour changes

- A stream that ends cleanly without its terminal event (Anthropic `message_stop`, OpenAI `[DONE]`) yields `Error::UnexpectedEnd` from `stream()` and `events()`; before, it ended as if complete. An OpenAI stream that sent a `finish_reason` but no `[DONE]` is complete. `TextStream` ends with `Error::Truncated` when the answer reaches `max_tokens`, after the last text delta.
- The agent keeps a reply's text beside its tool calls: the assistant message holds the text first, then the `ToolUse` blocks; before, the text was dropped. A reply with no text and no tool call stores no assistant message.
- `OpenAi` refuses a request with a `CacheBreakpoint` with `Error::Unsupported("prompt caching is not available with OpenAI")` before sending it; before, it dropped the breakpoints silently.
- `info().name` of an `OpenAi` model is `"openai"`; before, it was `"openai-compatible"`.
- A `Batches` from `Bedrock::anthropic()` answers `Error::Unsupported` on `submit`, `status`, `results` and `cancel` before any request: Bedrock has no message batches.

### Changed (breaking)

- The request and result types are `#[non_exhaustive]`: the structs `CompletionRequest`, `CacheBreakpoint`, `Message`, `ToolDefinition`, `ToolUseCall`, `Usage`, `Completion`, `ProviderInfo`, `ToolOutput`, `Transcript`, `TurnSummary` and `ToolCall`, and the enums `ModelSelection`, `BreakpointPosition` and `MessageRole`. Their fields stay public and readable; outside the crate they are built through the constructors in the Migration table, and a `match` on the three enums needs a `_` arm. `Content` is unchanged.
- `StreamEvent::ToolUse(ToolUseCall)` and `StreamEvent::Usage(Usage)` are tuple variants carrying the result types.
- `ProviderInfo` holds `name` and `model` only; `supports_tools` and `supports_streaming` are gone.
- `OpenAi`, the OpenAI-compatible endpoint, is built through `OpenAiBuilder`: `OpenAi::builder(url)` with `credential`, `timeouts`, `retry` and `http_client`; the setters `api_key`, `timeouts` and `retry` on the built client are gone, and `api_key(..)` became `credential(..)`.
- `ConversationRuntime::run_turn` folds each reply through the same fold as `complete`: `Truncated` fires at `MessageStop`, and a stream that ends before `MessageStop` is `RuntimeError::Model(Error::UnexpectedEnd)`.
- The agent has one `Tool` trait (`definition`, `call`), one `Toolbox` trait (`definitions`, `call(name, input)`), one `ToolOutput` and `Tools`, a toolbox of boxed tools built with `Tools::new().with(tool)`; both `ToolExecutor` traits and `ToolExecResult` are gone, and `agent::tool` is no longer a public module.
- `ConversationRuntime` owns its model and toolbox: `ConversationRuntime::builder(llm, toolbox)` with `system`, `max_tokens`, `max_iterations`, `tool_concurrency`, `max_tool_output_chars` and `on_delta`, then `build()`; `new(llm, toolbox)` keeps the defaults. `with_defaults`, `with_max_iterations`, `with_tool_concurrency` and `LoopDefaults` are gone.
- `run_turn(prompt)` takes only the prompt; streamed text goes to `RuntimeBuilder::on_delta`.
- Each file tool has one constructor, `XTool::new(sandbox)`; `with_sandbox` and `Default` are gone, so every file tool is sandboxed.
- `ToolError` moved from `agent` to `fs_tools`; `Execution` and the context-free `Io(io::Error)` are gone, and it names what failed (`InvalidInput`, `NotAFile`, `Io { path, source }`, `NotFound`, `NotUnique`, `Regex`, `Glob`, besides `Sandbox` and `InputTooLarge`).
- The modules `fs_tools::grep_tool` and `fs_tools::glob_tool` are renamed `fs_tools::grep` and `fs_tools::glob`.
- File-tool input is parsed into typed structs and each `input_schema` is derived with `schemars`. The text the model reads on failure changed: an input that does not parse is `invalid input: <serde message>`, and the `Error: ` prefix is gone. Grep's unused `glob` input left its schema.
- Glob without a `path` matches relative patterns against the sandbox root, not the working directory.
- `fs-tools` depends on `schemars`, `tempfile` and `walkdir` instead of `uuid`; Edit's atomic write uses `tempfile`.
- `CompletionRequest` has a new public field, `response_schema: Option<ResponseSchema>`, set with the `response_schema(..)` setter.
- The five file tools are one block, `fs_tools::Files`: `Tools::new().with_all(Files::new(sandbox))`, narrowed with `.only(["read", "edit"])`. `ReadTool`, `WriteTool`, `EditTool`, `GrepTool`, `GlobTool`, their modules and `From<Result<String, ToolError>> for ToolOutput` are gone. The tool names are lowercase: `read`, `write`, `edit`, `grep`, `glob`; inputs, schemas, descriptions and error texts are unchanged.
- The repository is a Cargo workspace under `crates/`: `llm-provider`, `llm-provider-macros` and `llm-provider-tools`; the `agent` feature depends on the macros. A git dependency on the repository still names the crate: `llm-provider = { git = "https://github.com/appdoo/llm-provider" }`.
- `schemars` is optional, behind the new `schema` feature, which `agent` turns on; level 0 no longer depends on it (it was optional under `fs-tools`).
- Message batches are behind the `batch` feature, off by default: `Batches` (from `Anthropic::batches(model)`), `BatchProvider`, `BatchRequest`, `BatchId`, `BatchStatus`, `BatchState`, `BatchCounts`, `BatchResult`, `BatchOutcome`, `Error::BatchNotEnded` and the `RequestError` variants `EmptyBatch`, `InvalidCustomId` and `DuplicateCustomId`.
- `ApiError` keeps the provider's `error.type` (`error_type()`), and its `Display` is `API error (<kind>, <status>, <error.type>): <message>`, leaving out a missing status or type; `Error::Api` displays it as is.
- An OpenAI-compatible in-stream error reads its kind from `error.type` like an Anthropic one (`invalid_request_error` is `ErrorKind::InvalidRequest`).

### Changed

- The README is a quick start: why, install and features, four short examples, the examples table and where the manual lives.
- The manual moved into rustdoc: protocols, credentials, timeouts, retry and size caps in the `provider` module doc; the JSON Schema subset of structured outputs on `Request::send_as`; tools, composition and turn limits in the `agent` module doc; the advisor evidence on `Advisor`; sandbox limits on `Sandbox`; the 10 MiB read limit on `Files`. The crate root has its own doc with the levels instead of including the README.
- `LlmProvider::stream` writes its return type out in full; `EventStream` stays the public alias for the same type, so implementations need no change.

### Added

- Endpoints and models: `Bedrock` (`Bedrock::new(key, region)`, `Bedrock::builder(key, region)` with `timeouts`, `retry`, `http_client` and `base_url`) hands out `.anthropic()` and `.openai()`, which share one HTTP client, retry policy and credential. `Anthropic` hands out `.llm(model)` and, with `batch`, `.batches(model)`; `OpenAi` (`OpenAi::new(key)`, `OpenAi::at(url)`, `OpenAi::builder(url)`) hands out `.llm(model)` and `.embedder(model)`. Models made from one endpoint share its connection pool.
- `Llm`, one model type for every protocol: `ask`, `request`, `stream` and `send_json`, `model()`, and `Llm::from_provider` for a custom `LlmProvider`.
- Embeddings: the `Embed` trait (`embed(texts) -> Vec<Vec<f32>>`, vectors in input order) and the `Embedder` model, sent to `POST {base}/embeddings` under the endpoint's retry policy; `Embedder::from_embed` for a custom `Embed`.
- Root exports `Bedrock`, `OpenAi`, `Llm`, `Embed`, `Embedder`, `ModelBuilder` and `Protocol`; `Batches` and the builders stay under `provider::`.
- Model id constants on `Anthropic` (`CLAUDE_HAIKU_5_5`, `CLAUDE_SONNET_5_5`, `CLAUDE_OPUS_5_5`, `CLAUDE_FABLE_5_1`) and on the new `Bedrock` (`EU_CLAUDE_HAIKU_5_5`, `EU_CLAUDE_SONNET_5_5`, `EU_CLAUDE_OPUS_5_5`, `KIMI_K3`).
- `examples/`: thirteen runnable programs, one per topic — `ask`, `typed_answer`, `local_model`, `agent_tools`, `shared_state`, `tool_by_hand`, `file_agent`, `advisor`, `critics`, `delegation`, `mailbox`, `sandbox` (offline) and `two_providers`. CI builds them all and runs `sandbox`.
- Typed answers: `Request::send_as::<T>(schema)` for `T: DeserializeOwned`, with `ResponseSchema::new(name, schema)`; with the `schema` feature, `Request::send_json::<T>()` for `T: DeserializeOwned + JsonSchema` and `ResponseSchema::of::<T>()`, which derives the shape. Anthropic uses structured outputs (`output_config.format` with type `json_schema`, no beta header, no forced tool, so models that reject `tool_choice` `tool` work), with the schema made conformant: every object gets `additionalProperties: false`, `minimum`, `maximum`, `multipleOf`, `minLength`, `maxLength`, `maxItems` and `uniqueItems` are removed, and `minItems` above 1 becomes 1. OpenAI-compatible servers get a `json_schema` response format (`strict: false`). The answer is the response text. A refusal, an answer cut at `max_tokens`, or a missing or unparsable answer is `Error::InvalidAnswer` with `ErrorKind::InvalidResponse`.
- `llm_provider::JsonSchema` re-exports `schemars::JsonSchema` (feature `schema`).
- `#[tool]` (`llm_provider::agent::tool`) turns an `async fn` taking no argument or one `Deserialize + JsonSchema` struct and returning `Result<impl Display, impl Display>` into a `Tool`: the function stays as written and a unit struct named after it in CamelCase (`unix_time` → `UnixTime`) implements `Tool`, its doc comment the description, its argument's schema the input schema. `#[tool(name = "…")]` renames the tool the model sees.
- `#[tools]` (`llm_provider::agent::tools`) on an impl block turns its `#[tool]` methods into tools sharing one `Arc` of the value, and implements the new `IntoTools` trait; `Tools::with_all` adds a block, `Tools::only(names)` keeps the named tools.
- `OpenAiBuilder::http_client` sends through a caller-configured `reqwest::Client`.
- `LlmProvider` is implemented for `Box<P>` and `&P`, so `Llm::from_provider` takes a boxed provider or a borrowed one that lives long enough.

## [0.1.0] - 2026-11-07

The first release in this repository.

### Added

- `provider` (level 0): Anthropic Messages and OpenAI Chat Completions providers with streaming, retry, timeouts, prompt caching and message batches.
- `agent` (level 1, feature `agent`): `ConversationRuntime`, a turn loop that streams a completion, runs the requested tools and feeds the results back.
- `fs_tools` (feature `fs-tools`): Read, Write, Edit, Glob and Grep tools behind one path `Sandbox`.
