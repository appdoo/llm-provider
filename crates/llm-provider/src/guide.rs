//! The guide: one chapter per capability, each with the call, what it
//! returns, the knobs, the errors and a runnable example.
//!
//! Chapters 1 to 6 build on each other; the rest can be read in any order.
//! Every code block compiles as a doctest. A chapter whose feature is off
//! says which feature it needs.
//!
//! 1. [Ask](ch01_ask): one question, one answer.
//! 2. [Stream](ch02_stream): the answer as it is written, and every event.
//! 3. [A typed answer](ch03_typed_answer): the answer parsed into your type (`schema`).
//! 4. [A tool](ch04_tool): an async function the model calls (`agent`).
//! 5. [An advisor](ch05_advisor): a second model that reads the conversation (`agent`).
//! 6. [Agents that ask each other](ch06_agents_that_ask_each_other): an agent as a tool, agents over a channel (`agent`).
//! 7. [Endpoints](ch07_endpoints): Anthropic, Amazon Bedrock, OpenAI and local servers.
//! 8. [Prompt caching and cost](ch08_caching_and_cost): `CacheTtl`, `Usage` and `Prices`.
//! 9. [The agent loop and tool errors](ch09_agent_loop): the turn, its limits, and what a failed tool call becomes (`agent`).
//! 10. [Store and restore a session](ch10_store_and_restore): every model call as an event, and a session continued from them (`agent`).
//! 11. [Compaction](ch11_compaction): a long history replaced by a shorter one (`agent`).
//! 12. [Cancellation](ch12_cancellation): a turn or a request stopped by a token (`agent`).
//! 13. [Sampling, timeouts and retry](ch13_sampling_timeouts_retry): `Sampling`, `ProviderOptions` and `RetryPolicy`.
//! 14. [Models from configuration, the explicit form](ch14_configuration): a model named in code or by values.
//! 15. [Sandboxed file tools](ch15_file_tools): `read`, `write`, `edit`, `grep` and `glob` in one directory (`fs-tools`).
//! 16. [Embeddings and batches](ch16_embeddings_and_batches): texts into vectors, and requests answered later (`batch`).
//! 17. [MCP servers as tools](ch17_mcp_servers): the tools of an MCP server in an agent (`llm-provider-tools`).
//! 18. [Your own provider](ch18_own_provider): a model on a wire the crate does not speak.

#[doc = include_str!("../guide/01-ask.md")]
pub mod ch01_ask {}

#[doc = include_str!("../guide/02-stream.md")]
pub mod ch02_stream {}

#[cfg_attr(feature = "schema", doc = include_str!("../guide/03-typed-answer.md"))]
#[cfg_attr(
    not(feature = "schema"),
    doc = "A typed answer; needs the `schema` feature."
)]
pub mod ch03_typed_answer {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/04-tool.md"))]
#[cfg_attr(not(feature = "agent"), doc = "A tool; needs the `agent` feature.")]
pub mod ch04_tool {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/05-advisor.md"))]
#[cfg_attr(not(feature = "agent"), doc = "An advisor; needs the `agent` feature.")]
pub mod ch05_advisor {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/06-agents-that-ask-each-other.md"))]
#[cfg_attr(
    not(feature = "agent"),
    doc = "Agents that ask each other; needs the `agent` feature."
)]
pub mod ch06_agents_that_ask_each_other {}

#[doc = include_str!("../guide/07-endpoints.md")]
pub mod ch07_endpoints {}

#[doc = include_str!("../guide/08-caching-and-cost.md")]
pub mod ch08_caching_and_cost {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/09-agent-loop.md"))]
#[cfg_attr(
    not(feature = "agent"),
    doc = "The agent loop and tool errors; needs the `agent` feature."
)]
pub mod ch09_agent_loop {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/10-store-and-restore.md"))]
#[cfg_attr(
    not(feature = "agent"),
    doc = "Store and restore a session; needs the `agent` feature."
)]
pub mod ch10_store_and_restore {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/11-compaction.md"))]
#[cfg_attr(not(feature = "agent"), doc = "Compaction; needs the `agent` feature.")]
pub mod ch11_compaction {}

#[cfg_attr(feature = "agent", doc = include_str!("../guide/12-cancellation.md"))]
#[cfg_attr(
    not(feature = "agent"),
    doc = "Cancellation; needs the `agent` feature."
)]
pub mod ch12_cancellation {}

#[doc = include_str!("../guide/13-sampling-timeouts-retry.md")]
pub mod ch13_sampling_timeouts_retry {}

#[doc = include_str!("../guide/14-configuration.md")]
pub mod ch14_configuration {}

#[cfg_attr(feature = "fs-tools", doc = include_str!("../guide/15-file-tools.md"))]
#[cfg_attr(
    not(feature = "fs-tools"),
    doc = "Sandboxed file tools; needs the `fs-tools` feature."
)]
pub mod ch15_file_tools {}

#[cfg_attr(feature = "batch", doc = include_str!("../guide/16-embeddings-and-batches.md"))]
#[cfg_attr(
    not(feature = "batch"),
    doc = "Embeddings and batches; needs the `batch` feature."
)]
pub mod ch16_embeddings_and_batches {}

#[doc = include_str!("../guide/17-mcp-servers.md")]
pub mod ch17_mcp_servers {}

#[doc = include_str!("../guide/18-own-provider.md")]
pub mod ch18_own_provider {}
