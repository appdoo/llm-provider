# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [Unreleased]

### Added

- `examples/`: thirteen runnable programs, one per topic — `ask`, `typed_answer`, `local_model`, `agent_tools`, `shared_state`, `tool_by_hand`, `file_agent`, `advisor`, `critics`, `delegation`, `mailbox`, `sandbox` (offline) and `two_providers`. CI builds them all and runs `sandbox`.

### Changed

- The README is a quick start: why, install and features, four short examples, the examples table and where the manual lives.
- The manual moved into rustdoc: protocols, credentials, timeouts, retry and size caps in the `provider` module doc; the JSON Schema subset of structured outputs on `Request::send_as`; tools, composition and turn limits in the `agent` module doc; the advisor evidence on `Advisor`; sandbox limits on `Sandbox`; the 10 MiB read limit on `Files`. The crate root has its own doc with the levels instead of including the README.

### Changed (breaking)

- `StreamEvent::ToolUse(ToolUseCall)` and `StreamEvent::Usage(Usage)` are tuple variants carrying the result types.
- `ProviderInfo` holds `name` and `model` only; `supports_tools` and `supports_streaming` are gone.
- `OpenAiCompatible` is built through `OpenAiBuilder`: `OpenAiCompatible::builder(base_url, model)` with `credential`, `timeouts`, `retry` and `http_client`; the setters `api_key`, `timeouts` and `retry` on the client are gone, and `api_key(..)` became `credential(..)`.
- `ConversationRuntime::run_turn` folds each reply through the same fold as `complete`: `Truncated` fires at `MessageStop`, and a stream that ends before `MessageStop` is `RuntimeError::Provider(Error::UnexpectedEnd)`.
- The agent has one `Tool` trait (`definition`, `call`), one `Toolbox` trait (`definitions`, `call(name, input)`), one `ToolOutput` and `Tools`, a toolbox of boxed tools built with `Tools::new().with(tool)`; both `ToolExecutor` traits and `ToolExecResult` are gone, and `agent::tool` is no longer a public module.
- `ConversationRuntime` owns its provider and toolbox: `ConversationRuntime::builder(provider, toolbox)` with `system`, `max_tokens`, `max_iterations`, `tool_concurrency`, `max_tool_output_chars` and `on_delta`, then `build()`; `new(provider, toolbox)` keeps the defaults. `with_defaults`, `with_max_iterations`, `with_tool_concurrency` and `LoopDefaults` are gone.
- `run_turn(prompt)` takes only the prompt; streamed text goes to `RuntimeBuilder::on_delta`.
- Each file tool has one constructor, `XTool::new(sandbox)`; `with_sandbox` and `Default` are gone, so every file tool is sandboxed.
- `ToolError` moved from `agent` to `fs_tools`; `Execution` and the context-free `Io(io::Error)` are gone, and it names what failed (`InvalidInput`, `NotAFile`, `Io { path, source }`, `NotFound`, `NotUnique`, `Regex`, `Glob`, besides `Sandbox` and `InputTooLarge`).
- The modules `fs_tools::grep_tool` and `fs_tools::glob_tool` are renamed `fs_tools::grep` and `fs_tools::glob`.
- File-tool input is parsed into typed structs and each `input_schema` is derived with `schemars`. The text the model reads on failure changed: an input that does not parse is `invalid input: <serde message>`, and the `Error: ` prefix is gone. Grep's unused `glob` input left its schema.
- Glob without a `path` matches relative patterns against the sandbox root, not the working directory.
- `fs-tools` depends on `schemars`, `tempfile` and `walkdir` instead of `uuid`; Edit's atomic write uses `tempfile`.
- `CompletionRequest` has a new public field, `response_schema: Option<ResponseSchema>`; struct literals add `response_schema: None`.
- The five file tools are one block, `fs_tools::Files`: `Tools::new().with_all(Files::new(sandbox))`, narrowed with `.only(["read", "edit"])`. `ReadTool`, `WriteTool`, `EditTool`, `GrepTool`, `GlobTool`, their modules and `From<Result<String, ToolError>> for ToolOutput` are gone. The tool names are lowercase: `read`, `write`, `edit`, `grep`, `glob`; inputs, schemas, descriptions and error texts are unchanged.
- The repository is a Cargo workspace of `llm-provider` and `llm-provider-macros`; the `agent` feature depends on the macros.
- `schemars` is optional, behind the new `schema` feature, which `agent` turns on; level 0 no longer depends on it (it was optional under `fs-tools`).
- Message batches are behind the new `batch` feature, off by default: `BatchProvider`, `BatchRequest`, `BatchId`, `BatchStatus`, `BatchState`, `BatchCounts`, `BatchResult`, `BatchOutcome`, `impl BatchProvider for Anthropic`, `Error::BatchNotEnded` and the `RequestError` variants `EmptyBatch`, `InvalidCustomId` and `DuplicateCustomId`.
- The configuration-file factories moved from `provider` to the new `config` module, behind the new `config` feature, off by default: `llm_provider::config::{ProviderConfig, ProviderKind, ConfigError, create_provider, create_anthropic}`. `ConfigError` is no longer re-exported at the crate root, and `Error::Config` exists only with `config`. `Anthropic::new` and `OpenAiCompatible::new` are the default way to build a provider.
- `ApiError` keeps the provider's `error.type` (`error_type()`), and its `Display` is `API error (<kind>, <status>, <error.type>): <message>`, leaving out a missing status or type; `Error::Api` displays it as is.
- An OpenAI-compatible in-stream error reads its kind from `error.type` like an Anthropic one (`invalid_request_error` is `ErrorKind::InvalidRequest`).

### Added

- Typed answers: `Request::send_as::<T>(schema)` for `T: DeserializeOwned`, with `ResponseSchema::new(name, schema)`; with the `schema` feature, `Request::send_json::<T>()` for `T: DeserializeOwned + JsonSchema` and `ResponseSchema::of::<T>()`, which derives the shape. Anthropic uses structured outputs (`output_config.format` with type `json_schema`, no beta header, no forced tool, so models that reject `tool_choice` `tool` work), with the schema made conformant: every object gets `additionalProperties: false`, `minimum`, `maximum`, `multipleOf`, `minLength`, `maxLength`, `maxItems` and `uniqueItems` are removed, and `minItems` above 1 becomes 1. OpenAI-compatible servers get a `json_schema` response format (`strict: false`). The answer is the response text. A refusal, an answer cut at `max_tokens`, or a missing or unparsable answer is `Error::InvalidAnswer` with `ErrorKind::InvalidResponse`.
- `llm_provider::JsonSchema` re-exports `schemars::JsonSchema` (feature `schema`).
- `#[tool]` (`llm_provider::agent::tool`) turns an `async fn` taking no argument or one `Deserialize + JsonSchema` struct and returning `Result<impl Display, impl Display>` into a `Tool`: the function stays as written and a unit struct named after it in CamelCase (`unix_time` → `UnixTime`) implements `Tool`, its doc comment the description, its argument's schema the input schema. `#[tool(name = "…")]` renames the tool the model sees.
- `#[tools]` (`llm_provider::agent::tools`) on an impl block turns its `#[tool]` methods into tools sharing one `Arc` of the value, and implements the new `IntoTools` trait; `Tools::with_all` adds a block, `Tools::only(names)` keeps the named tools.
- `OpenAiBuilder::http_client` sends through a caller-configured `reqwest::Client`.
- `LlmProvider` is implemented for `Box<P>` and `&P`, so a runtime takes a boxed provider or a borrowed one.

## [0.1.0] - 2026-11-07

The first release in this repository.

### Added

- `provider` (level 0): Anthropic Messages and OpenAI Chat Completions providers with streaming, retry, timeouts, prompt caching and message batches.
- `agent` (level 1, feature `agent`): `ConversationRuntime`, a turn loop that streams a completion, runs the requested tools and feeds the results back.
- `fs_tools` (feature `fs-tools`): Read, Write, Edit, Glob and Grep tools behind one path `Sandbox`.
