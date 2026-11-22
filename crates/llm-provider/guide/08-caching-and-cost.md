A long prefix that stays the same between calls can be cached and read
back at a tenth of the input price; [`Usage`](crate::Usage) shows what was
written and read, and [`Prices`](crate::Prices) turns it into dollars.

# Prompt caching

[`cache(ttl)`](crate::Request::cache) on a request marks the last tool, the
system prompt and the last message; the provider caches the prefix up to
each mark for the [`CacheTtl`](crate::CacheTtl):
[`FiveMinutes`](crate::CacheTtl::FiveMinutes) or
[`OneHour`](crate::CacheTtl::OneHour). It works at Anthropic and on Amazon
Bedrock; OpenAI caches on its own and ignores the marks.

```rust
use llm_provider::{CacheTtl, Llm};

async fn answer(model: &Llm, handbook: &str, question: &str) -> Result<String, llm_provider::Error> {
    let reply = model
        .request(question)
        .system(handbook)
        .cache(CacheTtl::OneHour)
        .send()
        .await?;

    let usage = reply.usage;
    println!(
        "cache: written {} (1h {}), read {}",
        usage.cache_creation_input_tokens,
        usage.cache_creation_1h_input_tokens,
        usage.cache_read_input_tokens,
    );
    Ok(reply.text)
}
```

The prefix must stay the same byte for byte. Below the model's minimum size
(a few thousand tokens) the provider caches nothing and says nothing, so
check `Usage`.

The agent loop caches by default for five minutes: every model call marks
the tools, the system prompt and the last message, so the cache grows with
the history. `SessionBuilder::cache(CacheTtl::OneHour)` or `no_cache()`
changes that ([chapter 9](crate::guide::ch09_agent_loop)).

# When it pays

A cache write costs 1.25 times the input price (2 times for one hour), a
read 0.1 times. The cache pays when the next call comes within its
lifetime: right for a tool loop, where calls come seconds apart; wrong for a
chat with minutes between turns, which writes every time and reads nothing,
and should use `CacheTtl::OneHour` or no cache.

# What it returns

[`Usage`](crate::Usage), on every [`Completion`](crate::Completion) and
`StreamEvent::Usage`:

- `input_tokens`: input read without the cache;
- `output_tokens`: output written;
- `cache_creation_input_tokens`: input written to the cache;
- `cache_creation_1h_input_tokens`: the part of those written for one hour;
  a request for one hour that shows 0 here was granted five minutes;
- `cache_read_input_tokens`: input read from the cache.

Providers without caching report 0.

# Cost

[`usage.cost(&prices)`](crate::Usage::cost) converts a `Usage` to dollars.
[`Prices::per_million(input, output)`](crate::Prices::per_million) takes
the input and output price per million tokens and derives the cache prices
from the input price (write 1.25 times, or 2 times for one hour; read 0.1
times); [`cache_write_5m`](crate::Prices::cache_write_5m),
[`cache_write_1h`](crate::Prices::cache_write_1h) and
[`cache_read`](crate::Prices::cache_read) override each.
[`regional(factor)`](crate::Prices::regional) multiplies every price:
Amazon Bedrock bills the `eu.`, `us.` and `apac.` inference profiles at 1.10
times the global list price in every category.

The crate ships no price table, since prices change; you name them.

```rust
use llm_provider::{Prices, Usage};

// Claude Sonnet on Bedrock via an `eu.` profile: list price (2026-11-21) × 1.10.
let sonnet = Prices::per_million(3.0, 15.0).regional(1.10);

let usage = Usage::default().input_tokens(2_000).output_tokens(500);
let dollars = usage.cost(&sonnet);
assert!((dollars - 0.01485).abs() < 1e-9);
```

A `Usage` is per call, never a running total, so costs add. In the agent
loop `TurnSummary::usage` adds a turn's calls up, and every
`SessionEvent::ModelCall` carries its own call's `Usage`.

# Errors

Caching adds no error of its own on a plain request: a prefix too short to
cache is not an error. A request whose cache marks the API would refuse is
rejected before it is sent, as
[`Error::InvalidRequest`](crate::Error::InvalidRequest) with a
[`RequestError`](crate::provider::RequestError) (too many breakpoints, a
one-hour mark after a five-minute one). `cost` cannot fail.

# Example

[`examples/cache.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/cache.rs):
a one-hour prompt cache on Bedrock, written once, read back, `Usage`
printed.
[`examples/session_store.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/session_store.rs):
the cost of every model call of a session.

```sh
AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example cache
```
