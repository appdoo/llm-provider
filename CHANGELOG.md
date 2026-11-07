# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [0.1.0] - 2026-11-07

The first release in this repository.

### Added

- `provider` (level 0): Anthropic Messages and OpenAI Chat Completions providers with streaming, retry, timeouts, prompt caching and message batches.
- `agent` (level 1, feature `agent`): `ConversationRuntime`, a turn loop that streams a completion, runs the requested tools and feeds the results back.
- `fs_tools` (feature `fs-tools`): Read, Write, Edit, Glob and Grep tools behind one path `Sandbox`.
