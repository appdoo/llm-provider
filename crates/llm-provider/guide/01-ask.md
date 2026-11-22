One question, one answer: an endpoint hands out a model, and the model
answers.

# The call

An endpoint is where requests go and with which credential:
[`Anthropic::new(key)`](crate::Anthropic::new) for the Anthropic API.
[`llm(model)`](crate::Anthropic::llm) names a model on it and returns an
[`Llm`](crate::Llm). [`ask`](crate::Llm::ask) sends one user message and returns
the answer text.

```rust
use llm_provider::Anthropic;

async fn ask() -> Result<String, Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY")?;

    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_HAIKU_5_5);
    Ok(claude.ask("What is a borrow checker?").await?)
}
```

The crate reads no environment variable; the program above does, and hands
the key over. A plain string is an API key; an OAuth access token goes as
[`AuthMethod::Bearer`](crate::AuthMethod::Bearer).

The model ids are constants on the endpoint
([`Anthropic::CLAUDE_HAIKU_5_5`](crate::Anthropic::CLAUDE_HAIKU_5_5),
`CLAUDE_SONNET_5_5`, `CLAUDE_OPUS_5_5`, `CLAUDE_FABLE_5_1`), or any string
the endpoint knows. Clones of an `Llm` share one connection pool, and so do
all models made from one endpoint.

# A request with more to it

[`request(prompt)`](crate::Llm::request) starts a
[`Request`](crate::Request); its setters add what `ask` leaves out, and
[`send`](crate::Request::send) returns the whole
[`Completion`](crate::Completion): the text, the tool calls, the token
counts ([`Usage`](crate::Usage)) and why the model stopped.

```rust
use llm_provider::{Completion, Llm};

async fn brief(claude: &Llm) -> Result<Completion, llm_provider::Error> {
    claude
        .request("What is a lifetime?")
        .system("Answer in one sentence.")
        .max_tokens(200)
        .send()
        .await
}
```

# The knobs

- [`system`](crate::Request::system): the system prompt.
- [`max_tokens`](crate::Request::max_tokens): the longest answer (default
  4096).
- [`model`](crate::Request::model): another model on the same endpoint, for
  this request only.
- [`cache`](crate::Request::cache): prompt caching, see
  [chapter 8](crate::guide::ch08_caching_and_cost).
- [`sampling`](crate::Request::sampling): temperature, top-p, seed and stop
  sequences.

Other endpoints (Amazon Bedrock, OpenAI, a local server) hand out the same
`Llm`: [chapter 7](crate::guide::ch07_endpoints).

# Errors

Every call returns [`Error`](crate::Error). Branch on
[`kind()`](crate::Error::kind), an [`ErrorKind`](crate::ErrorKind), never on
the message text: `Auth` for a missing or refused credential,
`InvalidRequest` for a request the provider rejects, `RateLimited`,
`Overloaded`, `Server`, `Timeout` and `Network` for failures another attempt
may cure ([`is_retryable`](crate::Error::is_retryable)). A failure of that kind before the answer starts is retried (four attempts
by default, [`RetryPolicy`](crate::RetryPolicy)); when every attempt fails
you get `Error::RetriesExhausted` wrapping the last attempt's error, and its
`kind()` is that error's.

```rust
use llm_provider::{ErrorKind, Llm};

async fn answer_or_say_why(claude: &Llm) -> String {
    match claude.ask("What is a borrow checker?").await {
        Ok(answer) => answer,
        Err(error) if error.kind() == ErrorKind::Auth => "check the key".into(),
        Err(error) => format!("failed: {error}"),
    }
}
```

# Example

[`examples/ask.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/ask.rs):
one question, then a request with a system prompt and a token limit.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example ask
```
