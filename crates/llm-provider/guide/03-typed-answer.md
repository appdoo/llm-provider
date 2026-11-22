The answer parsed into your own type, held to its JSON schema by the
provider. Needs the `schema` feature.

# The call

[`send_json::<T>()`](crate::Request::send_json) derives the schema from `T`
([`ResponseSchema::of`](crate::ResponseSchema::of)), sends the request held
to it and parses the answer into a `T`. The type's doc comments are what the
model reads: they are the instructions for each field.

```rust
use llm_provider::{JsonSchema, Llm};
use serde::Deserialize;

/// What a text is about.
#[derive(Deserialize, JsonSchema)]
struct Summary {
    /// Two to four sentences.
    sentences: Vec<String>,
    /// Three to five key points.
    key_points: Vec<String>,
}

async fn summarize(claude: &Llm, text: &str) -> Result<Summary, llm_provider::Error> {
    claude.request(text).system("Be brief.").send_json::<Summary>().await
}
```

The derives expand to `serde` and `schemars` paths, so your crate depends
on both (`serde` with `derive`, `schemars = "1"`).
[`JsonSchema`](crate::JsonSchema) is re-exported at the crate root.

# A schema written by hand

[`send_as(schema)`](crate::Request::send_as) takes a
[`ResponseSchema`](crate::ResponseSchema) you build from a name and a JSON
value, for a schema decided at run time or a type without the derive.

```rust
use llm_provider::{Llm, ResponseSchema};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Primes {
    primes: Vec<u32>,
}

async fn primes(claude: &Llm) -> Result<Primes, llm_provider::Error> {
    let schema = ResponseSchema::new(
        "Primes",
        json!({
            "type": "object",
            "properties": { "primes": { "type": "array", "items": { "type": "integer" } } },
            "required": ["primes"]
        }),
    );
    claude.request("Name three primes.").send_as(schema).await
}
```

# What the provider receives

Anthropic holds the text to the schema with structured outputs; an
OpenAI-compatible server gets a `json_schema` response format. Structured
outputs take a subset of JSON Schema, so the schema sent has every object
closed (`additionalProperties: false`), `minimum`, `maximum`, `multipleOf`,
`minLength`, `maxLength`, `maxItems` and `uniqueItems` removed, and
`minItems` lowered to at most 1. Checks beyond that subset, such as "two to
four sentences", stay yours, after parsing.

# The knobs

The same setters as every request ([chapter 1](crate::guide::ch01_ask)):
`system`, `max_tokens`, `model`, `cache`, `sampling`. A long type needs a
`max_tokens` large enough for the whole answer.

# Errors

[`Error::InvalidAnswer`](crate::Error::InvalidAnswer) (kind
`InvalidResponse`) when the model refused, the answer stopped at the token
limit, or it is missing or does not parse into `T`; its `reason` says which,
and `answer` holds the first 200 characters. Every other
[`Error`](crate::Error) as for [`send`](crate::Request::send).

```rust
use llm_provider::{Error, Llm, ResponseSchema};

async fn checked(claude: &Llm, schema: ResponseSchema) -> Option<Vec<u32>> {
    match claude.request("Name three primes.").send_as::<Vec<u32>>(schema).await {
        Ok(primes) => Some(primes),
        Err(Error::InvalidAnswer { reason, .. }) => {
            eprintln!("no usable answer: {reason}");
            None
        }
        Err(other) => {
            eprintln!("{other}");
            None
        }
    }
}
```

# Example

[`examples/typed_answer.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/typed_answer.rs):
`send_as` with a hand-written schema, `send_json` with a derived one.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example typed_answer --features schema
```
