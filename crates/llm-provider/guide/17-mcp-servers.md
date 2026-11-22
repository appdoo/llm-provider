The tools of an MCP server, local over stdio or remote over Streamable
HTTP, as tools of an agent. They come from a second crate,
`llm-provider-tools`, whose documentation holds the code: its
[README](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider-tools/README.md)
and [docs.rs/llm-provider-tools](https://docs.rs/llm-provider-tools).
This chapter says what it does.

# Install

```toml
[dependencies]
llm-provider = { version = "0.3", features = ["agent"] }
llm-provider-tools = "0.3"
```

# The call

`McpServer::stdio(program, args)` names a local server, started as a child
process; `McpServer::http(url)?` names a remote one, with `.bearer(token)`
for its credential. `connect()` starts the server or opens the connection,
runs `initialize`, lists the tools and keeps the session; it returns
`McpTools`, which `Tools::with_all` adds to an agent
([chapter 4](crate::guide::ch04_tool)). Each MCP tool becomes one tool with
the server's name, description and input schema; `names()` lists them.

# The knobs

- `prefix("x")` names every tool `x_<name>`, so two servers with the same
  tool name fit in one agent.
- `timeout` bounds each call and `connect`, 60 seconds by default.
- `approve` sees each call's tool name and input before anything is sent;
  on `false` the server never sees the call. It runs on a blocking thread,
  so it may wait for a person.
- `env` passes a variable to a stdio server, held as a `SecretString`. The
  server's environment is cleared except `PATH`, `HOME`, `LANG`, `TERM` and
  what `env` passes.

A model provider accepts tool names of the characters `A-Z`, `a-z`, `0-9`,
`_` and `-`, at most 64 of them; every other character becomes `_`, and
calls still reach the server under its own name.

# Errors

A failed call (its timeout, a protocol error, a lost connection) answers the
model with an error result, and the turn goes on
([chapter 9](crate::guide::ch09_agent_loop)). A lost connection is not
reopened. `connect` fails with the crate's own `Error`: a URL it refuses,
a bearer token over plain `http` to a host that is not loopback, a program
that does not start, a refused credential, a failed handshake or tool
listing, the timeout, or a tool name too long or not unique.

Tool descriptions and results come from the server and reach the model as
data; connect to servers you trust. A stdio server runs with your
privileges, on Linux and macOS.

# Example

[`examples/fetch_agent.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider-tools/examples/fetch_agent.rs):
Claude summarizes a page through `mcp-server-fetch` (needs `uvx`).

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run -p llm-provider-tools --example fetch_agent
```
