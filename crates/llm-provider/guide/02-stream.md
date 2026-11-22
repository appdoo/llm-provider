The answer as it is written, piece by piece; and, when you need more than
text, every event of the answer: tool calls, token counts and why the model
stopped.

# The call

[`stream()`](crate::Request::stream) on a [`Request`](crate::Request)
returns a [`TextStream`](crate::TextStream): a `Stream` of
`Result<String, Error>`, one item per piece of text. `futures::StreamExt`
is your own import (`futures = "0.3"` in your dependencies).

```rust
use futures::StreamExt;
use llm_provider::Llm;

async fn story(claude: &Llm) -> Result<(), llm_provider::Error> {
    let mut text = claude.request("Tell a short story about a lighthouse.").stream();
    while let Some(delta) = text.next().await {
        print!("{}", delta?);
    }
    Ok(())
}
```

The request starts when the stream is first polled. Dropping the stream
closes the request.

# Every event

[`events()`](crate::Request::events) returns an
[`EventStream`](crate::EventStream) of
[`StreamEvent`](crate::StreamEvent)s, exactly as the provider yields them:
`ContentDelta` (a piece of text), `ToolUse` (a complete tool call),
`Usage` (token counts; the events of one answer add up), `Stop` (the
[`StopReason`](crate::provider::StopReason)) and `MessageStop` (the end).
`StreamEvent` is `#[non_exhaustive]`, so a `match` ends with `_`.

```rust
use futures::StreamExt;
use llm_provider::{Llm, StreamEvent, Usage};

async fn tokens(claude: &Llm) -> Result<Usage, llm_provider::Error> {
    let mut usage = Usage::default();
    let mut events = claude.request("Name three primes.").events();
    while let Some(event) = events.next().await {
        match event? {
            StreamEvent::ContentDelta(text) => print!("{text}"),
            StreamEvent::Usage(more) => {
                usage.input_tokens += more.input_tokens;
                usage.output_tokens += more.output_tokens;
            }
            _ => {}
        }
    }
    Ok(usage)
}
```

# The knobs

The same setters as [`send`](crate::Request::send) in
[chapter 1](crate::guide::ch01_ask): `system`, `max_tokens`, `model`,
`cache`, `sampling`; and [`tools`](crate::Request::tools) to offer tool
definitions, whose calls arrive as `ToolUse` events. Running the tools and
sending their results back is the agent's job
([chapter 4](crate::guide::ch04_tool)).

# Errors

An item is an [`Error`](crate::Error) when the request or the stream fails;
an error is yielded once and ends the stream. An error before the first
content event is retried as the endpoint's
[`RetryPolicy`](crate::RetryPolicy) says; after text has arrived nothing is
retried. A `TextStream` reports [`Error::Truncated`](crate::Error::Truncated)
after the last piece when the answer stopped at `max_tokens`. A stream that
ends before its `MessageStop` is
[`Error::UnexpectedEnd`](crate::Error::UnexpectedEnd); one silent for
longer than the endpoint allows is
[`Error::IdleTimeout`](crate::Error::IdleTimeout).

# Example

[`examples/stream.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/stream.rs):
a story token by token, then the token usage from the event stream.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example stream
```
