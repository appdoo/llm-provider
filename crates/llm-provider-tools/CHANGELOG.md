# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

## [Unreleased]

### Added

- `mcp::McpServer`: connect to an MCP server over stdio (`McpServer::stdio`) or Streamable HTTP (`McpServer::http`, with `bearer`), with a per-call `timeout`, a name `prefix` and an `approve` hook.
- `mcp::McpTools`: the server's tools as `llm-provider` tools (`IntoTools`), each with the server's name, description and input schema.
- A spawned server's environment is cleared except `PATH`, `HOME`, `LANG`, `TERM` and its `env` entries; its stderr is relayed with the server's name.
