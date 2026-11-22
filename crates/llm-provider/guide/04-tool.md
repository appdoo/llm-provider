A tool is an async function the model may call; the agent loop runs it
when the model asks and hands the result back. Needs the `agent` feature.

# The call

[`#[tool]`](crate::agent::tool) on an `async fn` keeps the function as
written and adds a unit struct named after it in CamelCase (`unix_time` →
`UnixTime`) that implements [`Tool`](crate::agent::Tool).
[`Tools::new().with(..)`](crate::agent::Tools::with) collects tools, and
[`Session::new(llm, tools)`](crate::agent::Session::new) is an agent that
offers them to the model. [`run_turn`](crate::agent::Session::run_turn)
sends the prompt, runs every tool call the model makes, feeds the results
back, and repeats until the model answers without one.

```rust
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use llm_provider::agent::{tool, Session, Tools};
use llm_provider::Llm;

/// Current time in seconds since 1970-01-01 UTC.
#[tool]
async fn unix_time() -> Result<String, SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs().to_string())
}

async fn timed(claude: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let mut agent = Session::new(claude, Tools::new().with(UnixTime));

    let turn = agent.run_turn("What is the Unix time now?").await?;
    Ok(turn.response)
}
```

# What the model sees

- The doc comment is the tool's description; the function name is its name,
  unless `#[tool(name = "…")]` sets another.
- The one argument, a struct deriving `Deserialize` and
  [`JsonSchema`](crate::JsonSchema), is the input schema; its field doc
  comments are what the model reads about each field. A tool may also take
  no argument.
- The `Ok` value is the result text. The `Err` value is the text of a
  failed call: the model reads it, marked as an error, and the turn goes on.
  An input that does not parse into the argument is a failed call too
  (`invalid input: …`).

```rust
use std::convert::Infallible;

use llm_provider::agent::{tool, Tools};
use llm_provider::JsonSchema;
use serde::Deserialize;

/// Two numbers to add.
#[derive(Deserialize, JsonSchema)]
struct Pair {
    /// The first number.
    a: i64,
    /// The second number.
    b: i64,
}

/// Adds two integers.
#[tool]
async fn add(pair: Pair) -> Result<i64, Infallible> {
    Ok(pair.a + pair.b)
}

let tools = Tools::new().with(Add);
# let _ = tools;
```

The derives expand to `serde` and `schemars` paths, so your crate depends
on both (`serde` with `derive`, `schemars = "1"`).

# What it returns

[`TurnSummary`](crate::agent::TurnSummary): `response`, the text of every
model reply in the turn; `tool_calls`, each call's name, input and the
output the model read; `iterations`, the model calls made; `usage`, their
token counts added up.

# More than one tool

Tools that share state (an HTTP client, a channel) are methods of one
struct in a [`#[tools]`](crate::agent::tools) impl block, added with
[`Tools::with_all`](crate::agent::Tools::with_all);
[`Tools::only`](crate::agent::Tools::only) keeps the named ones. A tool
whose name or schema is decided at run time implements
[`Tool`](crate::agent::Tool) by hand. A tool that wants the calling
conversation, the turn's cancel token or its number takes a last parameter
`&CallContext<'_>` ([`CallContext`](crate::agent::CallContext)).

# Errors

Tool names and inputs come from the model; a tool validates its input before
acting on it. A failed, unknown, panicking or hung tool call never ends the
turn; the model reads an error result. What does end a turn is a
[`SessionError`](crate::agent::SessionError):
[chapter 9](crate::guide::ch09_agent_loop).

# Example

[`examples/agent_tools.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/agent_tools.rs):
an agent with two `#[tool]` functions.
[`examples/shared_state.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/shared_state.rs):
a `#[tools]` block sharing one HTTP client.
[`examples/tool_by_hand.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/tool_by_hand.rs):
a `Tool` implemented by hand.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example agent_tools --features agent
```
