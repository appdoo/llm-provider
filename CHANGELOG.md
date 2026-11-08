# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [Unreleased]

### Changed (breaking)

- `StreamEvent::ToolUse(ToolUseCall)` and `StreamEvent::Usage(Usage)` are tuple variants carrying the result types.
- `ProviderInfo` holds `name` and `model` only; `supports_tools` and `supports_streaming` are gone.
- `OpenAiCompatible` is built through `OpenAiBuilder`: `OpenAiCompatible::builder(base_url, model)` with `credential`, `timeouts`, `retry` and `http_client`; the setters `api_key`, `timeouts` and `retry` on the client are gone, and `api_key(..)` became `credential(..)`.
- `ConversationRuntime::run_turn` folds each reply through the same fold as `complete`: `Truncated` fires at `MessageStop`, and a stream that ends before `MessageStop` is `RuntimeError::Provider(Error::UnexpectedEnd)`.
- The agent has one `Tool` trait (`definition`, `call`), one `Toolbox` trait (`definitions`, `call(name, input)`), one `ToolOutput` and `Tools`, a toolbox of boxed tools built with `Tools::new().with(tool)`; both `ToolExecutor` traits and `ToolExecResult` are gone, and `agent::tool` is no longer a public module.
- `ConversationRuntime` owns its provider and toolbox: `ConversationRuntime::builder(provider, toolbox)` with `system`, `max_tokens`, `max_iterations`, `tool_concurrency`, `max_tool_output_chars` and `on_delta`, then `build()`; `new(provider, toolbox)` keeps the defaults. `with_defaults`, `with_max_iterations`, `with_tool_concurrency` and `LoopDefaults` are gone.
- `run_turn(prompt)` takes only the prompt; streamed text goes to `RuntimeBuilder::on_delta`.
- The file tools are built with `XTool::new(sandbox)`; `XTool::default()` builds one without a sandbox.
- An OpenAI-compatible in-stream error reads its kind from `error.type` like an Anthropic one (`invalid_request_error` is `ErrorKind::InvalidRequest`).

### Added

- `OpenAiBuilder::http_client` sends through a caller-configured `reqwest::Client`.
- `LlmProvider` is implemented for `Box<P>` and `&P`, so a runtime takes the boxed provider `create_provider` returns, or a borrowed one.

## [0.1.0] - 2026-11-07

The first release in this repository.

### Added

- `provider` (level 0): Anthropic Messages and OpenAI Chat Completions providers with streaming, retry, timeouts, prompt caching and message batches.
- `agent` (level 1, feature `agent`): `ConversationRuntime`, a turn loop that streams a completion, runs the requested tools and feeds the results back.
- `fs_tools` (feature `fs-tools`): Read, Write, Edit, Glob and Grep tools behind one path `Sandbox`.
