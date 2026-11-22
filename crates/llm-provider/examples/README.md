# Examples

Each file in this directory is a complete program. The Claude examples read the key from `ANTHROPIC_API_KEY` (the example does; the crate never reads an environment variable) and exit 2 with a usage line without it. The local-model examples take `[BASE_URL] [MODEL]`, default `http://localhost:11434/v1` and `llama3.2`.

The last column is the guide chapter that explains what the example uses.

| Example | What it shows | Run | Guide |
|---|---|---|---|
| [`ask`](ask.rs) | one question, then a request with a system prompt and a token limit | `cargo run --example ask` | [1](https://docs.rs/llm-provider/latest/llm_provider/guide/ch01_ask/index.html) |
| [`stream`](stream.rs) | a story token by token, then the token usage from the event stream | `cargo run --example stream` | [2](https://docs.rs/llm-provider/latest/llm_provider/guide/ch02_stream/index.html) |
| [`typed_answer`](typed_answer.rs) | `send_as` with a hand-written schema, `send_json` with a derived one | `cargo run --example typed_answer --features schema` | [3](https://docs.rs/llm-provider/latest/llm_provider/guide/ch03_typed_answer/index.html) |
| [`local_model`](local_model.rs) | a local OpenAI-compatible server, no key | `cargo run --example local_model` | [7](https://docs.rs/llm-provider/latest/llm_provider/guide/ch07_endpoints/index.html) |
| [`agent_tools`](agent_tools.rs) | an agent with two `#[tool]` functions | `cargo run --example agent_tools --features agent` | [4](https://docs.rs/llm-provider/latest/llm_provider/guide/ch04_tool/index.html) |
| [`shared_state`](shared_state.rs) | a `#[tools]` block sharing one HTTP client, next to `Files` cut down with `only` | `cargo run --example shared_state --features fs-tools` | [4](https://docs.rs/llm-provider/latest/llm_provider/guide/ch04_tool/index.html) |
| [`tool_by_hand`](tool_by_hand.rs) | a `Tool` implemented by hand, named at run time | `cargo run --example tool_by_hand --features agent` | [4](https://docs.rs/llm-provider/latest/llm_provider/guide/ch04_tool/index.html) |
| [`file_agent`](file_agent.rs) | a local model that fixes typos with the file tools, streamed | `cargo run --example file_agent --features fs-tools` | [15](https://docs.rs/llm-provider/latest/llm_provider/guide/ch15_file_tools/index.html) |
| [`advisor`](advisor.rs) | Opus 5.5 fixes a bug, Fable 5.1 advises; advice and tally printed | `cargo run --example advisor --features fs-tools` | [5](https://docs.rs/llm-provider/latest/llm_provider/guide/ch05_advisor/index.html) |
| [`critics`](critics.rs) | a writer with two critics whose disagreement stays visible | `cargo run --example critics --features agent` | [5](https://docs.rs/llm-provider/latest/llm_provider/guide/ch05_advisor/index.html) |
| [`delegation`](delegation.rs) | a lead that delegates to two agents (`into_tool`) and asks an advisor | `cargo run --example delegation --features fs-tools` | [6](https://docs.rs/llm-provider/latest/llm_provider/guide/ch06_agents_that_ask_each_other/index.html) |
| [`mailbox`](mailbox.rs) | two agents in two tasks talking over a channel | `cargo run --example mailbox --features agent` | [6](https://docs.rs/llm-provider/latest/llm_provider/guide/ch06_agents_that_ask_each_other/index.html) |
| [`sandbox`](sandbox.rs) | the file tools called directly; a read outside the root fails (offline) | `cargo run --example sandbox --features fs-tools` | [15](https://docs.rs/llm-provider/latest/llm_provider/guide/ch15_file_tools/index.html) |
| [`bedrock`](bedrock.rs) | Claude and Kimi K3 on Amazon Bedrock with one Bedrock API key | `AWS_BEARER_TOKEN_BEDROCK=… cargo run --example bedrock` | [7](https://docs.rs/llm-provider/latest/llm_provider/guide/ch07_endpoints/index.html) |
| [`cache`](cache.rs) | a one-hour prompt cache on the Anthropic API or Bedrock: written once, read back, `Usage` printed | `ANTHROPIC_API_KEY=… cargo run --example cache` | [8](https://docs.rs/llm-provider/latest/llm_provider/guide/ch08_caching_and_cost/index.html) |
| [`own_provider`](own_provider.rs) | an own provider on `core`: Ollama's native `/api/generate` (needs a local Ollama) | `OLLAMA_MODEL=… cargo run --example own_provider` | [18](https://docs.rs/llm-provider/latest/llm_provider/guide/ch18_own_provider/index.html) |
| [`session_store`](session_store.rs) | a session stored as JSON lines, restored from its events, compacted through a summarizer | `cargo run --example session_store --features agent` | [10](https://docs.rs/llm-provider/latest/llm_provider/guide/ch10_store_and_restore/index.html) |
| [`two_providers`](two_providers.rs) | Claude plans, a local model writes | `cargo run --example two_providers --features agent` | [7](https://docs.rs/llm-provider/latest/llm_provider/guide/ch07_endpoints/index.html) |
| [`verify`](verify.rs) | every route and model: ask, stream, a typed answer and a tool turn, one table row each | `cargo run --example verify --all-features` | - |

The MCP example, `fetch_agent`, lives in [`llm-provider-tools`](../../llm-provider-tools/examples/fetch_agent.rs) ([chapter 17](https://docs.rs/llm-provider/latest/llm_provider/guide/ch17_mcp_servers/index.html)).
