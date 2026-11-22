The agent loop runs a turn: it sends the prompt, runs the tools the model
asks for, feeds their results back, and repeats until the model answers
without a tool call. A tool that fails never ends the turn; the model reads
what went wrong. Needs the `agent` feature.

# The call

[`Session::new(llm, tools)`](crate::agent::Session::new) or
[`Session::builder(llm, tools)`](crate::agent::Session::builder) for the
knobs; [`run_turn(prompt)`](crate::agent::Session::run_turn) runs one turn
and adds the prompt, the replies and the tool results to the
[`history`](crate::agent::Session::history). The next turn continues the
same conversation.

```rust
use std::time::Duration;

use llm_provider::agent::{Session, Tools};
use llm_provider::Llm;

async fn two_turns(llm: Llm, tools: Tools) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = Session::builder(llm, tools)
        .system("You fix typos in the files you are given.")
        .max_iterations(10)
        .tool_timeout(Duration::from_secs(30))
        .on_delta(|text| print!("{text}"))
        .build();

    let first = agent.run_turn("Fix the typos in README.md.").await?;
    println!("\n{} tool calls, {} model calls", first.tool_calls.len(), first.iterations);

    agent.run_turn("Now the same for CHANGELOG.md.").await?;
    Ok(())
}
```

# What it returns

[`TurnSummary`](crate::agent::TurnSummary):

- `response`: the text of every model reply in the turn, joined;
- `tool_calls`: every [`ToolCall`](crate::agent::ToolCall) in call order,
  with its name, the model's input and the
  [`ToolOutput`](crate::agent::ToolOutput) the model read (`content`,
  `is_error`);
- `iterations`: how many model calls the turn made;
- `usage`: their token counts added up
  ([chapter 8](crate::guide::ch08_caching_and_cost)).

# The knobs

On [`SessionBuilder`](crate::agent::SessionBuilder):

- [`system`](crate::agent::SessionBuilder::system): the system prompt sent
  with every request.
- [`max_tokens`](crate::agent::SessionBuilder::max_tokens): each request's
  token limit (default 8192).
- [`max_iterations`](crate::agent::SessionBuilder::max_iterations): model
  calls per turn (default 25).
- [`tool_concurrency`](crate::agent::SessionBuilder::tool_concurrency):
  tool calls run at once (default 4).
- [`max_tool_output_chars`](crate::agent::SessionBuilder::max_tool_output_chars):
  the longest tool result (default 100 000); longer output keeps its head and
  tail.
- [`tool_timeout`](crate::agent::SessionBuilder::tool_timeout): how long one
  tool call may run (default 120 seconds).
- [`on_delta`](crate::agent::SessionBuilder::on_delta): each piece of text
  as it arrives.
- [`cache`](crate::agent::SessionBuilder::cache) and
  [`no_cache`](crate::agent::SessionBuilder::no_cache): prompt caching, on
  for five minutes by default.

# Tool errors

Each of these becomes a result marked `is_error` that the model reads; the
turn goes on, and the model can try again or answer without the tool:

- the tool returns `Err`: its text;
- the input does not parse into the tool's argument: `invalid input: …`;
- the model names a tool that is not there: `unknown tool …`;
- the tool panics: `tool … panicked`;
- the tool runs past `tool_timeout`: it is dropped, and the result says
  `tool … timed out after … s`.

Every one of them is in `tool_calls`, so a caller can see them after the
turn.

```rust
use llm_provider::agent::TurnSummary;

fn failed_calls(turn: &TurnSummary) -> Vec<&str> {
    turn.tool_calls
        .iter()
        .filter(|call| call.output.is_error)
        .map(|call| call.output.content.as_str())
        .collect()
}
```

# Errors that end a turn

[`SessionError`](crate::agent::SessionError): `Model` (a model call failed,
with the provider's [`Error`](crate::Error)), `MaxIterations` (the model
still asked for tools after the last allowed call), `Truncated` (a reply
stopped at `max_tokens`), `Interrupted` (a cancel), `Store` (the event
receiver is gone) and `Compact` (the compaction callback failed). Each
carries the turn so far, [`turn()`](crate::agent::SessionError::turn): its
text, the tool calls that finished, the model calls and their usage, so a
failed turn still returns its work. Only a successful turn adds to the
history.

```rust
use llm_provider::agent::{Session, SessionError, Tools};

async fn attempt(agent: &mut Session<Tools>) -> String {
    match agent.run_turn("Plan the refactor.").await {
        Ok(turn) => turn.response,
        Err(SessionError::MaxIterations { limit, turn, .. }) => {
            format!("stopped after {limit} calls; so far: {}", turn.response)
        }
        Err(error) => {
            let turn = error.turn();
            format!("{error} after {} tool calls", turn.tool_calls.len())
        }
    }
}
```

# Example

[`examples/agent_tools.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/agent_tools.rs):
an agent with two `#[tool]` functions.
[`examples/file_agent.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/file_agent.rs):
a local model that fixes typos with the file tools, streamed.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example agent_tools --features agent
```
