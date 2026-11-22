How a request samples its answer, how long the crate waits for the
network, and how often it tries again.

# Sampling

[`sampling(Sampling)`](crate::Request::sampling) on a request sets the
temperature, top-p, seed and stop sequences. A value left unset is the
server's default.

```rust,no_run
use llm_provider::{Llm, Sampling};

async fn steady(model: &Llm) -> Result<String, llm_provider::Error> {
    let sampling = Sampling::new().temperature(0.2).top_p(0.9).stop(["</answer>"]);

    let reply = model
        .request("Name three prime numbers.")
        .sampling(sampling)
        .send()
        .await?;

    Ok(reply.text)
}
```

- [`temperature`](crate::Sampling::temperature) and
  [`top_p`](crate::Sampling::top_p) must be finite.
- [`seed`](crate::Sampling::seed): the accepted range is each server's
  (llama-server reads a `u32`, vLLM an `i64`, Ollama a Go `int`). The
  Anthropic Messages API has none.
- [`stop`](crate::Sampling::stop): a second call replaces the list. A finish
  by a stop sequence reports `StopReason::EndTurn`; the wire does not say
  which sequence fired.

The values go to the provider as they are. A model that does not take one
answers with its own error (Claude Sonnet 5.5, for one: "`temperature` is
deprecated for this model", `ErrorKind::InvalidRequest`).

# Timeouts

[`ProviderOptions`](crate::ProviderOptions) holds two limits: 30 seconds to
connect, and 90 seconds of silence before the response headers or between
two chunks of the body. A long answer that keeps streaming never times out.
Set them on the endpoint builder with
[`timeouts`](crate::provider::EndpointBuilder::timeouts).

# Retry

A failure another attempt may cure (rate limit, overload, a server error, a
timeout, a network failure) is retried before the first content event of an
answer; once text has arrived, an error ends the stream. The wait doubles
from one second up to a cap of 60 seconds, with jitter, and a
`Retry-After` from the server is honoured; one longer than the cap gives
up. [`RetryPolicy`](crate::RetryPolicy) sets it, four attempts by default,
on the endpoint builder with
[`retry`](crate::provider::EndpointBuilder::retry).

```rust
use std::time::Duration;

use llm_provider::{Anthropic, Llm, ProviderOptions, RetryPolicy};

fn patient(key: &str) -> Llm {
    let timeouts = ProviderOptions::default()
        .connect_timeout(Duration::from_secs(5))
        .idle_gap(Duration::from_secs(120));
    let retry = RetryPolicy::default().max_attempts(6).cap(Duration::from_secs(120));

    Anthropic::builder(key)
        .timeouts(timeouts)
        .retry(retry)
        .build()
        .llm(Anthropic::CLAUDE_HAIKU_5_5)
}
```

[`RetryPolicy::none()`](crate::RetryPolicy::none) makes one attempt. A
model built from values takes the same limits as `max_attempts`,
`connect_timeout_secs` and `stream_idle_timeout_secs`
([chapter 14](crate::guide::ch14_configuration)).

# Errors

- A non-finite temperature or top-p, and a seed on the Anthropic Messages
  API, are [`Error::InvalidRequest`](crate::Error::InvalidRequest) before
  any I/O.
- Silence past the idle gap is
  [`Error::IdleTimeout`](crate::Error::IdleTimeout), of kind
  `ErrorKind::Timeout`.
- Retries that ran out return
  [`Error::RetriesExhausted`](crate::Error::RetriesExhausted) with the
  number of attempts and the last attempt's error;
  [`kind()`](crate::Error::kind) reports the last attempt's kind, so a
  branch on `ErrorKind` sees through it.

