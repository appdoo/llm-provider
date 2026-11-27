An advisor is one more tool: a second model that reads the whole
conversation of the agent that calls it and answers with advice. Sonnet
works, Opus advises, when Sonnet asks. Needs the `agent` feature.

# The call

[`Advisor::new(llm)`](crate::agent::Advisor::new) is a
[`Tool`](crate::agent::Tool) on its own model. The calling agent decides
when to call it; [`CONSULT_ADVISOR`](crate::agent::CONSULT_ADVISOR), put in
its system prompt, tells it when: before it commits to an approach, and
before it declares the task done. Nothing adds that prompt automatically.

```rust
use llm_provider::agent::{Advisor, Session, Tools, CONSULT_ADVISOR};
use llm_provider::Llm;

async fn advised(worker: Llm, stronger: Llm) -> Result<String, Box<dyn std::error::Error>> {
    let tools = Tools::new().with(Advisor::new(stronger));
    let mut coder = Session::builder(worker, tools).system(CONSULT_ADVISOR).build();

    let turn = coder.run_turn("Why does median() return the wrong value?").await?;
    Ok(turn.response)
}
```

The model's input to the tool is ignored. The advisor receives the calling
conversation rendered as text by
[`Transcript::render`](crate::agent::Transcript::render), as data in a user
message, and its answer is the tool result the calling model reads.

# What it returns

The advice is a tool call like any other: it appears in
[`TurnSummary::tool_calls`](crate::agent::TurnSummary::tool_calls) with the
advisor's name and its answer as the output.

```rust
use llm_provider::agent::{Session, Tools};

async fn advice(coder: &mut Session<Tools>) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let turn = coder.run_turn("Fix the failing test.").await?;
    Ok(turn
        .tool_calls
        .into_iter()
        .filter(|call| call.name == "advisor")
        .map(|call| call.output.content)
        .collect())
}
```

# The knobs

- [`name`](crate::agent::Advisor::name): the tool name the calling model
  sees (default `advisor`).
- [`description`](crate::agent::Advisor::description): the tool
  description it sees.
- [`system`](crate::agent::Advisor::system): the advisor's persona, sent as
  its system prompt.
- [`max_tokens`](crate::agent::Advisor::max_tokens): the longest answer
  (default 2048).
- [`max_uses`](crate::agent::Advisor::max_uses): how many calls one turn of
  the calling agent may make; later calls fail without a request.

Several advisors, each with its own `name` and `system` persona, act as
critics; their disagreement stays visible in `tool_calls`.

```rust
use llm_provider::agent::{Advisor, Session, Tools, CONSULT_ADVISOR};
use llm_provider::Llm;

fn writer(main: Llm, critic: Llm) -> Session<Tools> {
    let strict = Advisor::new(critic.clone())
        .name("strict_critic")
        .system("You check every claim against the text.")
        .max_uses(2);
    let reader = Advisor::new(critic)
        .name("reader")
        .system("You read as someone new to the topic.")
        .max_uses(2);

    Session::builder(main, Tools::new().with(strict).with(reader))
        .system(CONSULT_ADVISOR)
        .build()
}
```

# Errors

A failed advisor call (its model fails, or `max_uses` is spent) is an error
result starting with `advisor unavailable: `, which the calling model reads;
the turn goes on. The calling turn fails
only as any turn does: [chapter 9](crate::guide::ch09_agent_loop).

# Example

[`examples/advisor.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/advisor.rs):
Sonnet 5.5 fixes a bug, Opus 5.5 advises; advice and tally printed.
[`examples/critics.rs`](https://github.com/appdoo/llm-provider/blob/main/crates/llm-provider/examples/critics.rs):
a writer with two critics whose disagreement stays visible.

```sh
ANTHROPIC_API_KEY=sk-ant-… cargo run --example advisor --features fs-tools
```
