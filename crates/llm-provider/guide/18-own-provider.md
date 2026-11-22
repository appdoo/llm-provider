A server whose wire the crate does not speak can still be a model: a type
that implements [`LlmProvider`](crate::LlmProvider), built on the same core
the crate's own providers use. Everything that takes an
[`Llm`](crate::Llm), agents included, takes it.

# The call

An own provider implements two methods:
[`stream(request)`](crate::LlmProvider::stream) returns the reply as an
[`EventStream`](crate::EventStream), and
[`info()`](crate::LlmProvider::info) names the provider and the model for
errors and logs. [`Llm::from_provider`](crate::Llm::from_provider) makes it
a model.

The [`core`](crate::core) module does the rest:
[`Transport`](crate::core::Transport) POSTs a JSON body with your headers,
and [`send_body`](crate::core::Transport::send_body) hands the reply's
bytes to a decoder from [`decode`](crate::core::decode). The example speaks
Ollama's native `/api/generate`, one JSON object per line:

```rust,no_run
use llm_provider::core::{decode, Transport};
use llm_provider::provider::{
    CompletionRequest, EventStream, LlmProvider, ProviderInfo, StreamEvent,
};
use llm_provider::Llm;
use serde_json::json;

struct Ollama {
    transport: Transport,
    model: String,
}

impl LlmProvider for Ollama {
    // The reply as events: one text event per NDJSON line.
    fn stream(&self, request: CompletionRequest) -> EventStream {
        let prompt = request.last_user_text().unwrap_or_default();
        let body = json!({ "model": self.model, "prompt": prompt, "stream": true });

        self.transport.send_body("/api/generate", body, |bytes| {
            decode::lines(bytes, |line| {
                let v: serde_json::Value = serde_json::from_str(line)?;
                Ok(v["response"].as_str().map(StreamEvent::text))
            })
        })
    }

    // Who this provider is, for errors and logs.
    fn info(&self) -> ProviderInfo {
        ProviderInfo::new("ollama", &self.model)
    }
}

async fn goats() -> Result<(), Box<dyn std::error::Error>> {
    let transport = Transport::at("http://localhost:11434")?; // .bearer(key) when needed

    let llm = Llm::from_provider(Ollama { transport, model: "qwen3".into() });
    println!("{}", llm.ask("How many goats?").await?);
    Ok(())
}
```

# The decoders

- [`decode::lines`](crate::core::decode::lines) hands each line to your
  closure, which returns the event it carries, if any. Server-sent events
  work as they are: the `data:` prefix is stripped, other fields and blank
  lines are skipped, and `data: [DONE]` ends the stream. A
  `serde_json::Error` converts with `?`.
- [`decode::anthropic`](crate::core::decode::anthropic),
  [`decode::openai_chat`](crate::core::decode::openai_chat) and
  [`decode::openai_completions`](crate::core::decode::openai_completions)
  for a server that speaks one of those wires.

# The knobs

[`Transport::at(url)`](crate::core::Transport::at) takes an absolute `http`
or `https` URL, with the default timeouts and retry policy
([chapter 13](crate::guide::ch13_sampling_timeouts_retry)).
[`bearer(token)`](crate::core::Transport::bearer) sends
`Authorization: Bearer …` with every request, and
[`header(name, value)`](crate::core::Transport::header) any other header;
both values are held as secrets and never printed. Clones of a `Transport`
share its connection pool.

An embedding model of your own implements [`Embed`](crate::Embed) and
becomes an [`Embedder`](crate::Embedder) with
[`Embedder::from_embed`](crate::Embedder::from_embed).

# Errors

`Transport::at` returns a [`ConfigError`](crate::config::ConfigError) for
a URL that is not absolute `http` or `https`. `send_body` retries an error
before the first content event under the retry policy; a failure to send is
the stream's only item. A reply with no bytes is
[`Error::EmptyResponse`](crate::Error::EmptyResponse), and an error your
closure returns ends the stream. `Llm::from_provider` adds no retry of its
own.

# Example

[`examples/own_provider.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/own_provider.rs):
the program above; it needs an Ollama on `localhost:11434`.

```sh
cargo run --example own_provider
```
