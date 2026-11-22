An endpoint is where requests go and with which credential. Every endpoint
hands out the same [`Llm`](crate::Llm), so the rest of a program does not
change with the provider.

# Anthropic

[`Anthropic::new(key)`](crate::Anthropic::new) sends to
`https://api.anthropic.com`. A plain string is an API key (`x-api-key`); an
OAuth access token goes as
[`AuthMethod::Bearer`](crate::AuthMethod::Bearer).

```rust
use llm_provider::Anthropic;

async fn ask() -> Result<String, Box<dyn std::error::Error>> {
    let key = std::env::var("ANTHROPIC_API_KEY")?;

    let claude = Anthropic::new(key).llm(Anthropic::CLAUDE_SONNET_5_5);
    Ok(claude.ask("What is a borrow checker?").await?)
}
```

[`Anthropic::builder(key)`](crate::Anthropic::builder) takes another
address with `base_url(url)?`, for an Anthropic-compatible server.

# Amazon Bedrock

[`Bedrock::new(key, region)?`](crate::Bedrock::new) sends to
`https://bedrock-runtime.{region}.amazonaws.com` with a Bedrock API key.
It has two routes: [`anthropic()`](crate::Bedrock::anthropic), the Anthropic
Messages API under `/anthropic`, for Claude; and
[`openai()`](crate::Bedrock::openai), the OpenAI-compatible API under
`/openai/v1`, for other models such as
[`Bedrock::KIMI_K3`](crate::Bedrock::KIMI_K3). A Claude id on the wrong
route is answered with 400.

One endpoint hands out several models, which share its connection pool. A
common pair is a main model for the work and a smaller one for side tasks,
such as compaction summaries or a researcher agent: here Opus and Sonnet
through the `eu.` inference profiles.

```rust
use llm_provider::{Bedrock, Llm};

/// The main model and the side model, both on Bedrock in Frankfurt.
fn models() -> Result<(Llm, Llm), Box<dyn std::error::Error>> {
    let key = std::env::var("AWS_BEARER_TOKEN_BEDROCK")?;

    let bedrock = Bedrock::new(key, "eu-central-1")?.anthropic();
    let main = bedrock.llm(Bedrock::EU_CLAUDE_OPUS_5_5);
    let side = bedrock.llm(Bedrock::EU_CLAUDE_SONNET_5_5);
    Ok((main, side))
}
```

The side model must be a profile enabled in the account and in the same
geography (`eu.`); a valid id can still answer 403 until the account enables
the model. Regional profiles (`eu.`, `us.`, `apac.`) cost 1.10 times the
list price: [chapter 8](crate::guide::ch08_caching_and_cost).

# OpenAI and local servers

[`OpenAi::new(key)`](crate::OpenAi::new) sends to
`https://api.openai.com/v1`. [`OpenAi::at(url)?`](crate::OpenAi::at) sends
to an OpenAI-compatible server you name (Ollama, vLLM) without a
credential; [`OpenAi::builder(url)?`](crate::OpenAi::builder) adds one with
`.credential(key)`.

```rust
use llm_provider::{Llm, OpenAi};

fn local() -> Result<Llm, llm_provider::config::ConfigError> {
    Ok(OpenAi::at("http://localhost:11434/v1")?.llm("qwen3"))
}

fn served(key: &str) -> Result<Llm, llm_provider::config::ConfigError> {
    Ok(OpenAi::builder("http://gpu-box:8000/v1")?
        .credential(key)
        .build()
        .llm("qwen3"))
}
```

# The knobs

Every endpoint builder (`Anthropic::builder`, `OpenAi::builder`,
`Bedrock::builder`) also takes
[`timeouts`](crate::provider::EndpointBuilder::timeouts)
([`ProviderOptions`](crate::ProviderOptions): 30 s to connect, 90 s of
silence), [`retry`](crate::provider::EndpointBuilder::retry)
([`RetryPolicy`](crate::RetryPolicy): 4 attempts) and
[`http_client`](crate::provider::EndpointBuilder::http_client) for a
`reqwest::Client` of your own. Models named by values from a configuration
file are [`Llm::builder`](crate::Llm::builder).

# Errors

The constructors check what they are given; nothing is built from an invalid
value, and no request is sent.
[`ConfigError::InvalidOrigin`](crate::config::ConfigError::InvalidOrigin):
an address that is not an absolute URL;
[`ConfigError::UnsupportedScheme`](crate::config::ConfigError::UnsupportedScheme):
a scheme other than `http` or `https`;
[`ConfigError::InvalidRegion`](crate::config::ConfigError::InvalidRegion):
a Bedrock region not of the form `eu-central-1`. Neither address error
prints the address. A `ConfigError` converts into
[`Error`](crate::Error) with `?`.

A wrong or missing key shows on the first request, as an
[`Error`](crate::Error) of kind [`ErrorKind::Auth`](crate::ErrorKind::Auth).

# Example

[`examples/bedrock.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/bedrock.rs):
Claude and Kimi K3 on Amazon Bedrock with one Bedrock API key.
[`examples/local_model.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/local_model.rs):
a local OpenAI-compatible server, no key.
[`examples/session_store.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/session_store.rs):
Opus for the session and Sonnet for the compaction summaries on Bedrock.

```sh
AWS_BEARER_TOKEN_BEDROCK=ABSK… cargo run --example bedrock
cargo run --example local_model -- http://localhost:11434/v1 llama3.2
```
