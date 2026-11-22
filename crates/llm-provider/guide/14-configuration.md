A model can be named in code, with the endpoint's type, or by values known
only at run time, from a configuration file or a command-line flag. Both
make the same [`Llm`](crate::Llm).

# The explicit form

An endpoint built by hand reads left to right: protocol, address,
credential, model name. Every model made from one endpoint shares its
connection pool ([chapter 7](crate::guide::ch07_endpoints)).

```rust
use llm_provider::{Llm, OpenAi};

fn qwen(key: &str) -> Result<Llm, llm_provider::Error> {
    Ok(OpenAi::builder("http://gpu-box:8000/v1")? // protocol, address
        .credential(key)                         // credential
        .build()
        .llm("qwen3"))                           // model name
}
```

# From values

[`Llm::builder(model)`](crate::Llm::builder) and
[`Embedder::builder(model)`](crate::Embedder::builder) return a
[`ModelBuilder`](crate::ModelBuilder) that takes the protocol, the address
and the rest as values. Each `build()` makes its own endpoint, so models
built this way do not share a connection pool; the typed form above does.

```rust
use llm_provider::{Llm, ModelBuilder, Protocol};
use secrecy::SecretString;

fn models(key: SecretString, text: &str) -> Result<(Llm, Llm), llm_provider::Error> {
    let qwen = Llm::builder("qwen3")             // the model name
        .protocol(Protocol::OpenAi)              // the protocol
        .endpoint("http://gpu-box:8000/v1")      // the address
        .api_key(key.clone())                    // the credential, always set in code
        .build()?;

    // The same values from a file; the credential never comes from it.
    let opus = serde_json::from_str::<ModelBuilder<Llm>>(text)?.api_key(key).build()?;
    Ok((qwen, opus))
}
```

# The knobs

On [`ModelBuilder`](crate::ModelBuilder), each with its file key:

- [`protocol`](crate::ModelBuilder::protocol), `protocol` (or `default`):
  [`Protocol`](crate::Protocol), `anthropic` (or `claude`) or `openai`;
  default `anthropic`. From a command-line flag, `str::parse` reads the name
  in any case.
- [`endpoint`](crate::ModelBuilder::endpoint), `endpoint` (or `base_url`):
  the address. The Anthropic API and Amazon Bedrock default to their public
  origins; an OpenAI-compatible server needs one.
- [`bedrock`](crate::ModelBuilder::bedrock), `region`: Amazon Bedrock in
  that region, such as `eu-central-1`, for either protocol.
- `model`: the model id. Without one, the Anthropic API takes Claude Sonnet
  5.5; Bedrock and an OpenAI-compatible server need one.
- [`max_attempts`](crate::ModelBuilder::max_attempts),
  [`connect_timeout_secs`](crate::ModelBuilder::connect_timeout_secs),
  [`stream_idle_timeout_secs`](crate::ModelBuilder::stream_idle_timeout_secs):
  retry and timeouts, defaults 4, 30 and 90
  ([chapter 13](crate::guide::ch13_sampling_timeouts_retry)).
- [`api_key`](crate::ModelBuilder::api_key),
  [`oauth_token`](crate::ModelBuilder::oauth_token) or
  [`credential`](crate::ModelBuilder::credential): set in code only. No file
  key holds a credential, and serialising never writes one.

```json
{ "protocol": "anthropic", "region": "eu-central-1", "model": "eu.anthropic.claude-opus-5-5" }
```

# Errors

`build()` sends nothing. It returns
[`Error::Config`](crate::Error::Config) with a
[`ConfigError`](crate::config::ConfigError): `MissingCredential` (the
Anthropic API and Bedrock need one), `MissingEndpoint`, `MissingModel`,
`InvalidRegion`, `InvalidOrigin` or `UnsupportedScheme`. A protocol name
that `str::parse` does not know is `UnknownProtocol`; in a file it fails
the `serde` parse. `Embedder::builder` with the Anthropic protocol is
[`Error::Unsupported`](crate::Error::Unsupported): it has no embedding
models.

```rust
use llm_provider::config::ConfigError;
use llm_provider::{Error, Llm, Protocol};

let missing = Llm::builder("qwen3").protocol(Protocol::OpenAi).build();
assert!(matches!(
    missing,
    Err(Error::Config(ConfigError::MissingEndpoint { .. }))
));
```

