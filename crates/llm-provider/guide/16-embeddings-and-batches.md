Two calls besides chat: texts into vectors, and many requests sent as one
batch that the provider answers later. Embeddings need no feature; batches
need the `batch` feature.

# Embeddings

An OpenAI-compatible endpoint hands out an embedding model with
[`embedder(model)`](crate::OpenAi::embedder), an
[`Embedder`](crate::Embedder). [`embed(&texts)`](crate::Embed::embed),
from the [`Embed`](crate::Embed) trait, returns one vector per text, in
input order; an empty slice returns no vectors and sends no request.

```rust,no_run
use llm_provider::{Embed, OpenAi};

async fn vectors(texts: &[String]) -> Result<Vec<Vec<f32>>, llm_provider::Error> {
    let bge = OpenAi::at("http://localhost:11434/v1")?.embedder("bge-m3");

    bge.embed(texts).await
}
```

Clones of an `Embedder`, and every model from one endpoint, share its
connection pool and retry policy.
[`Embedder::builder(model)`](crate::Embedder::builder) names one by values
([chapter 14](crate::guide::ch14_configuration)), and
[`Embedder::from_embed`](crate::Embedder::from_embed) wraps an embedder of
your own, without retry. An [`Anthropic`](crate::Anthropic) endpoint has no
embedding models: it offers no `embedder`, and `Embedder::builder` with the
Anthropic protocol is [`Error::Unsupported`](crate::Error::Unsupported).

# Batches

[`Anthropic::batches(model)`](crate::Anthropic::batches) returns
[`Batches`](crate::provider::Batches), a
[`BatchProvider`](crate::provider::BatchProvider) with four calls:
[`submit`](crate::provider::BatchProvider::submit) sends the requests as
one batch and returns its [`BatchId`](crate::provider::BatchId);
[`status`](crate::provider::BatchProvider::status) returns its
[`BatchState`](crate::provider::BatchState) (`InProgress`, `Canceling`,
`Ended`) and how many requests ended which way;
[`results`](crate::provider::BatchProvider::results) returns every outcome
of an ended batch; [`cancel`](crate::provider::BatchProvider::cancel) asks
the server to stop it.

Each request is a [`CompletionRequest`](crate::CompletionRequest) under a
`custom_id` you choose, a
[`BatchRequest`](crate::provider::BatchRequest); each
[`BatchResult`](crate::provider::BatchResult) names the `custom_id` it
answers.

```rust,no_run
use llm_provider::provider::{BatchOutcome, BatchProvider, BatchRequest, BatchState};
use llm_provider::{Anthropic, CompletionRequest, Message};

async fn summaries(key: &str, texts: &[String]) -> Result<(), llm_provider::Error> {
    let batches = Anthropic::new(key).batches(Anthropic::CLAUDE_HAIKU_5_5);

    let mut requests = Vec::new();
    for (n, text) in texts.iter().enumerate() {
        let request = CompletionRequest::new([Message::user(text.as_str())]).system("Summarize in one line.");
        requests.push(BatchRequest::new(format!("text_{n}"), request)?);
    }
    let id = batches.submit(requests).await?;

    // Later, once the batch has ended:
    if batches.status(&id).await?.state() == BatchState::Ended {
        for result in batches.results(&id).await? {
            match result.outcome() {
                BatchOutcome::Succeeded(completion) => println!("{}: {}", result.custom_id(), completion.text),
                other => println!("{}: {other:?}", result.custom_id()),
            }
        }
    }
    Ok(())
}
```

A [`BatchOutcome`](crate::provider::BatchOutcome) is `Succeeded` with the
`Completion`, `Errored` with the provider's error, `Malformed` when the
result line could not be read, `Canceled` or `Expired`.

# Errors

- A `custom_id` that does not match `^[a-zA-Z0-9_-]{1,64}$`, an empty batch
  and a repeated `custom_id` are
  [`Error::InvalidRequest`](crate::Error::InvalidRequest), before any
  network I/O.
- `results` on a batch still running is
  [`Error::BatchNotEnded`](crate::Error::BatchNotEnded).
- On Amazon Bedrock every batch call is
  [`Error::Unsupported`](crate::Error::Unsupported), before any request.
- `cancel` is never retried, so it is sent at most once.

