A turn or a request can be stopped from outside with a
`tokio_util::sync::CancellationToken`. What was done before the cancel comes
back; the history stays as it was. The agent part needs the `agent`
feature; a cancellable request needs none.

# A turn

[`run_turn_cancellable(prompt, &token)`](crate::agent::Session::run_turn_cancellable)
runs a turn as [`run_turn`](crate::agent::Session::run_turn) does, until
the token fires. On cancel it drops the model stream and the running tool
calls, writes nothing more to the history, and rolls the history back to
before the call.

```rust,no_run
use llm_provider::agent::{Session, SessionError, Tools};
use tokio_util::sync::CancellationToken;

async fn plan(agent: &mut Session<Tools>, token: &CancellationToken) {
    match agent.run_turn_cancellable("Plan the refactor.", token).await {
        Ok(turn) => println!("{}", turn.response),
        Err(SessionError::Interrupted { turn, .. }) => {
            println!("cut after {} tool calls: {}", turn.tool_calls.len(), turn.response);
        }
        Err(error) => println!("{error}"),
    }
}
```

[`SessionError::Interrupted`](crate::agent::SessionError::Interrupted)
carries the turn so far: all its text, earlier iterations included, and the
tool calls that completed; a call cut by the cancel is not listed. A
compaction at the start of the turn stays
([chapter 11](crate::guide::ch11_compaction)).

A dropped `run_turn` future rolls the history back as well; the token is
the way to keep the partial work.

# Tools

A tool sees the turn's token as
[`CallContext::cancel`](crate::agent::CallContext::cancel). The runtime
drops the tool's own future at once on cancel; a tool that spawned work (a
task, a child process) hands the token to it, or waits with
[`context.cancelled().await`](crate::agent::CallContext::cancelled).

# A request

[`send_cancellable(&token)`](crate::Request::send_cancellable) sends a
request as [`send`](crate::Request::send) does, unless the token fires
first. A cancel returns [`Error::Interrupted`](crate::Error::Interrupted)
with the text received so far and drops the stream, which closes the
request; nothing is retried afterwards. A token already cancelled sends
nothing, and a finished answer that is ready together with the cancel wins.

```rust,no_run
use llm_provider::{Error, Llm};
use tokio_util::sync::CancellationToken;

async fn draft(model: &Llm, token: &CancellationToken) -> Result<String, Error> {
    match model.request("Write a long story.").send_cancellable(token).await {
        Ok(reply) => Ok(reply.text),
        Err(Error::Interrupted { partial_text, .. }) => Ok(partial_text),
        Err(error) => Err(error),
    }
}
```

