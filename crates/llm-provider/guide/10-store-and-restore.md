A session reports what it does as events over a channel you give it; the
crate stores nothing. To continue a session later, hand the stored events
back, and the crate folds them into the history. Needs the `agent` feature.

# The call

[`events(tx)`](crate::agent::SessionBuilder::events) on the builder takes a
`tokio::sync::mpsc::Sender` of [`SessionEvent`](crate::agent::SessionEvent).
[`restore(events)`](crate::agent::SessionBuilder::restore) takes the events
your store returned. Each event serializes with serde, so one
`serde_json::to_string` makes it a line of your database.

```rust,no_run
use llm_provider::agent::{Session, SessionEvent, Tools};
use llm_provider::Llm;

async fn store_and_restore(llm: Llm, stored: Vec<SessionEvent>) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<SessionEvent>(64);
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let line = serde_json::to_string(&event).expect("an event serializes");
            println!("{line}"); // the app's database goes here
        }
    });

    let mut agent = Session::builder(llm, Tools::new())
        .restore(stored) // the events your database returned
        .events(tx)
        .build();

    agent.run_turn("Where were we?").await?;
    Ok(())
}
```

The system prompt and the tools come from the app on restore, set on the
builder as for a new session; the events do not set them.

# What it sends

Three kinds of [`SessionEvent`](crate::agent::SessionEvent), each with the
session id, a `seq` (0 for the first event, +1 per event, never reset by a
restore) and the turn, counted from 1:

- `ModelCall`: one model call, sent after it ended, whether it succeeded or
  failed: the model, the provider, the start and duration in milliseconds,
  the stop reason, the messages the call added to the history, its own
  [`Usage`](crate::Usage) and, if it failed, the error's kind and message.
  The session's first event also carries the system prompt and the tool
  names (`setup`).
- `Compacted`: the history was replaced by the compaction callback, with
  the message counts before and after and the new history in full
  ([chapter 11](crate::guide::ch11_compaction)).
- `TurnEnded`: a turn ended, with `ok` and the turn's usage added up.

A `Usage` per call prices with [`Usage::cost`](crate::Usage::cost)
([chapter 8](crate::guide::ch08_caching_and_cost)).

```rust
use llm_provider::agent::SessionEvent;
use llm_provider::Prices;

fn dollars(events: &[SessionEvent], prices: &Prices) -> f64 {
    events
        .iter()
        .map(|event| match event {
            SessionEvent::ModelCall { usage, .. } => usage.cost(prices),
            _ => 0.0,
        })
        .sum()
}
```

[`Session::session_id`](crate::agent::Session::session_id) names the
session before its first event, for a file name or a database key.

# What a restore makes of them

Give [`restore`](crate::agent::SessionBuilder::restore) all events of one
session, or those from its last `Compacted` on, in any order; the crate
sorts them by `seq`. The session starts with the last compacted history,
then the messages of the model calls of every turn that ended ok. A turn
without an ok `TurnEnded` (failed, cancelled, or cut by a process that
died) did not join the history and is left out.

The restored session keeps the stored id; its next event follows the
highest `seq`, its next turn the highest turn plus one. No events give a
fresh session. Events of two session ids: the id of the highest-`seq` event
wins and the other events are ignored.

The stored format is a promise: a line written by this version reads back
in every later version of the crate.

# Errors

A session sends each event before it goes on, so a full channel slows it
down. A closed channel ends the turn with
[`SessionError::Store`](crate::agent::SessionError::Store) before a model
call goes unrecorded; the turn adds nothing to the history. `restore`
cannot fail.

# Example

[`examples/session_store.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/session_store.rs):
a session stored as JSON lines in a file, restored from it, the cost of
every model call printed, and one compaction through a summarizer.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example session_store --features agent
```
