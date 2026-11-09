# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [Unreleased]

### Added

- `mcp::McpServer`: connect to an MCP server over stdio (`McpServer::stdio`) or Streamable HTTP (`McpServer::http`, with `bearer`), with a per-call `timeout`, a name `prefix` and an `approve` hook.
- `mcp::McpTools`: the server's tools as `llm-provider` tools (`IntoTools`), each with the server's name, description and input schema.
- A spawned server's environment is cleared except `PATH`, `HOME`, `LANG`, `TERM` and its `env` entries; its stderr is relayed with the server's name.
- `timeout` also bounds `connect` (`initialize` and the tool listing); past it, `connect` fails with `Error::Timeout`.
- Tool names are offered to the model with every character outside `[A-Za-z0-9_-]` replaced by `_`; calls use the server's own name. A name over 64 characters or two names that become the same fail `connect` (`Error::NameTooLong`, `Error::NameCollision`).
- A spawned server is killed when its session is dropped without a running async runtime.
- `approve` runs on a blocking thread.
- Stdio servers run on Linux and macOS; Windows is unsupported in 0.x.
