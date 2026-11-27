# Changelog

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver, where a 0.x minor version may break the API.

0.3.0 is the first version on crates.io; 0.1.0 to 0.2.1 were git tags. The Breaking and Migration tables are for code written against those.

## [0.3.0](https://github.com/appdoo/llm-provider/compare/llm-provider-tools-v0.2.1...llm-provider-tools-v0.3.0) - 2026-11-27

### Breaking

- `McpServer::http(url)` → `McpServer::http(&url)?`. It takes `&str` and returns `Result`: anything but an absolute `http` or `https` URL is `Error::InvalidUrl`.
- `McpServer::http` refuses a URL with a username, password, query or fragment (`Error::InvalidUrl`, new variant). A key goes in `.bearer(token)`.
- An MCP tool's `Tool::call` takes `&CallContext<'_>` in place of `&Transcript<'_>` (llm-provider 0.3).

### Security

- `url` 2.5.9 is the minimum: earlier versions overflow the stack in `Url::origin()` on nested `blob:` URLs (RUSTSEC-2026-0592)
- A bearer token over plain `http` to a host that is not loopback is `Error::InsecureBearer`, refused by `connect` before any byte is sent.

## [0.2.1](https://github.com/appdoo/llm-provider/compare/llm-provider-tools-v0.1.0...llm-provider-tools-v0.2.1) - 2026-11-16

### Other

- security.sh schedule wording, core 0.2.0 date
- call ToolOutput::error directly; drop needless set_var in test
- tools at 0.2.0, workspace dependencies for llm-provider, rmcp, tokio-stream, trybuild
- tools joins the llm-provider version group; release only from the release PR; changelog fixes
- mcp tests: find the test_server example above the test binary (Cargo 1.100 build-dir layout)
- Build the request and result types through their constructors outside the crate
- llm-provider 0.2.0
- runtime, advisor and tools take the concrete Llm; from_provider and from_embed
- Anthropic and Bedrock as endpoints handing out models; Batches; every caller migrated

### Changed

- Depends on llm-provider 0.2: `McpTools` implements its `IntoTools`; migrate with the llm-provider 0.2 notes.

## [0.1.0] - 2026-11-09

### Changed

- The crate lives in the `llm-provider` workspace (`crates/llm-provider-tools`) and depends on `llm-provider` by path; install it from `https://github.com/appdoo/llm-provider`.

### Added

- `mcp::McpServer`: connect to an MCP server over stdio (`McpServer::stdio`) or Streamable HTTP (`McpServer::http`, with `bearer`), with a per-call `timeout`, a name `prefix` and an `approve` hook.
- `mcp::McpTools`: the server's tools as `llm-provider` tools (`IntoTools`), each with the server's name, description and input schema.
- A spawned server's environment is cleared except `PATH`, `HOME`, `LANG`, `TERM` and its `env` entries; its stderr is relayed with the server's name.
- `timeout` also bounds `connect` (`initialize` and the tool listing); past it, `connect` fails with `Error::Timeout`.
- Tool names are offered to the model with every character outside `[A-Za-z0-9_-]` replaced by `_`; calls use the server's own name. A name over 64 characters or two names that become the same fail `connect` (`Error::NameTooLong`, `Error::NameCollision`).
- A spawned server is killed when its session is dropped without a running async runtime.
- `approve` runs on a blocking thread.
- Stdio servers run on Linux and macOS; Windows is unsupported in 0.x.
