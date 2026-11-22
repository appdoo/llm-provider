An agent can be another agent's tool, keeping its own conversation; and
agents in separate tasks can talk through a channel. Needs the `agent`
feature.

# An agent as a tool

[`Session::into_tool(name, description)`](crate::agent::Session::into_tool)
turns an agent into an [`AgentTool`](crate::agent::AgentTool). The calling
model sends `{"prompt": "…"}`; the tool runs one turn of the wrapped agent
and answers with its response.

```rust
use llm_provider::agent::{Session, Tools};
use llm_provider::Llm;

async fn lead(opus: Llm, haiku: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let researcher = Session::new(haiku, Tools::new())
        .into_tool("researcher", "Answers one factual question. Give it the question.");
    let mut lead = Session::new(opus, Tools::new().with(researcher));

    Ok(lead.run_turn("Ask the researcher who built the first lighthouse.").await?.response)
}
```

The wrapped agent keeps its conversation across calls, so a second
question can refer to the first. Calls to one agent run one at a time;
calls to different agents run in parallel, up to
[`tool_concurrency`](crate::agent::SessionBuilder::tool_concurrency)
(default 4). The description is what the lead reads when it decides whom to
ask, so it names what the agent does and what to give it.

# Agents in separate tasks

Two agents that run on their own, each in its own task, talk through a
channel held by a [`#[tools]`](crate::agent::tools) block. No type of the
crate is involved beyond the tool macros.

```rust
use std::convert::Infallible;

use llm_provider::agent::{tools, Session, Tools};
use llm_provider::{JsonSchema, Llm};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};

/// One agent's end of the conversation with the other.
struct Mailbox {
    tx: mpsc::Sender<String>,
    rx: Mutex<mpsc::Receiver<String>>,
}

/// A message for the other agent.
#[derive(Deserialize, JsonSchema)]
struct Note {
    /// The message.
    text: String,
}

#[tools]
impl Mailbox {
    /// Sends a message to the other agent.
    #[tool]
    async fn send(&self, note: Note) -> Result<String, mpsc::error::SendError<String>> {
        self.tx.send(note.text).await.map(|()| "sent".into())
    }

    /// Waits for the other agent's next message.
    #[tool]
    async fn receive(&self) -> Result<String, Infallible> {
        let next = self.rx.lock().await.recv().await;
        Ok(next.unwrap_or_else(|| "(the other agent has left)".into()))
    }
}

fn agent(llm: Llm, mailbox: Mailbox) -> Session<Tools> {
    Session::new(llm, Tools::new().with_all(mailbox))
}
```

Two mailboxes whose channels cross (what one sends, the other receives)
give two agents a conversation; each runs its turns in its own
`tokio::spawn`.

# What it returns

The lead's [`TurnSummary::tool_calls`](crate::agent::TurnSummary::tool_calls)
lists every call to the wrapped agent, with the prompt as input and its
response as output. The wrapped agent's own tool calls stay in its own
turns.

# Errors

A failed turn of the wrapped agent is an error result the lead reads, with
the turn's error as its text; the lead's turn goes on. An input without a
`prompt` string is a failed call (`invalid input: …`).

# Example

[`examples/delegation.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/delegation.rs):
a lead that delegates to two agents and asks an advisor.
[`examples/mailbox.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/mailbox.rs):
two agents in two tasks talking over a channel.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example delegation --features fs-tools
```
