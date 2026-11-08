# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [Unreleased]

### Changed (breaking)

- `StreamEvent::ToolUse(ToolUseCall)` and `StreamEvent::Usage(Usage)` are tuple variants carrying the result types.
- `ProviderInfo` holds `name` and `model` only; `supports_tools` and `supports_streaming` are gone.
- `OpenAiCompatible` is built through `OpenAiBuilder`: `OpenAiCompatible::builder(base_url, model)` with `credential`, `timeouts`, `retry` and `http_client`; the setters `api_key`, `timeouts` and `retry` on the client are gone, and `api_key(..)` became `credential(..)`.
- An OpenAI-compatible in-stream error reads its kind from `error.type` like an Anthropic one (`invalid_request_error` is `ErrorKind::InvalidRequest`).

### Added

- `OpenAiBuilder::http_client` sends through a caller-configured `reqwest::Client`.

## [0.1.0] - 2026-11-07

The first release in this repository.

### Added

- `provider` (level 0): Anthropic Messages and OpenAI Chat Completions providers with streaming, retry, timeouts, prompt caching and message batches.
- `agent` (level 1, feature `agent`): `ConversationRuntime`, a turn loop that streams a completion, runs the requested tools and feeds the results back.
- `fs_tools` (feature `fs-tools`): Read, Write, Edit, Glob and Grep tools behind one path `Sandbox`.
