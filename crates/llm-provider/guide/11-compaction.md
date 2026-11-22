A long conversation fills the model's context. Compaction replaces the
history by a shorter one that your callback returns, such as a summary from
a smaller model. Needs the `agent` feature.

# The call

[`compact_when(threshold, callback)`](crate::agent::SessionBuilder::compact_when)
on the builder. At the start of a turn, when the last model call read
`threshold` or more input tokens (cache reads and writes included), the
session passes its history to `callback` and continues with the messages it
returns. It never compacts inside a turn's tool loop.

```rust,no_run
use llm_provider::agent::{Session, Tools};
use llm_provider::{Llm, Message};

fn long_running(llm: Llm, summarizer: Llm) -> Session {
    Session::builder(llm, Tools::new())
        .compact_when(150_000, move |history: Vec<Message>| {
            let summarizer = summarizer.clone();
            async move {
                let text: Vec<String> = history.iter().map(Message::text).collect();

                let summary = summarizer
                    .request(text.join("\n"))
                    .system("Summarize this conversation in a few sentences. Keep every number.")
                    .send()
                    .await?
                    .text;

                Ok(vec![Message::user(format!("Summary so far:\n{summary}"))])
            }
        })
        .build()
}
```

The callback is `FnMut(Vec<Message>)` returning a future of
`Result<Vec<Message>, CompactError>`; [`CompactError`](crate::agent::CompactError)
is any boxed error, so `?` works on the errors of your summarizer. The
crate ships no summarizer.

# The threshold

The threshold is an absolute token count the app chooses; the crate knows
no model's context window. It is compared with the last call's input, which
does not count that call's reply or the new prompt, so leave headroom below
the window. An endpoint that reports no usage never reaches the threshold.

A restored session takes the last call's input from its stored events, so
its first turn compacts when that call was over the threshold
([chapter 10](crate::guide::ch10_store_and_restore)).

# What it records

A compaction sends a `SessionEvent::Compacted` before the model call that
follows it, with the message counts before and after and the new history in
full; a stored session restores to that history. A compaction stays when its
turn then fails.

# Errors

A callback error ends the turn as
[`SessionError::Compact`](crate::agent::SessionError::Compact), with the
callback's error as its source and the history unchanged.

```rust
use llm_provider::agent::{Session, SessionError};

async fn turn(agent: &mut Session, prompt: &str) -> String {
    match agent.run_turn(prompt).await {
        Ok(turn) => turn.response,
        Err(SessionError::Compact { error, .. }) => format!("compaction failed: {error}"),
        Err(error) => format!("{error}"),
    }
}
```

# Example

[`examples/session_store.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/session_store.rs):
a threshold of 10 tokens, small enough to show a compaction through a
summary from a side model.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example session_store --features agent
```
