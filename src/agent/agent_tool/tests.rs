//! Tests for `AgentTool` (Spec 38, X-3..X-5).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::agent::test_support::{calls, text, tool_use, tool_use_with};
use crate::agent::{RuntimeError, Tools};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Content, Error, StreamEvent};

/// A lead whose model calls `worker` once with `prompt`, then answers
/// `lead done`.
fn lead_calling_once(prompt: &str) -> ScriptedProvider {
    let call = tool_use_with("c1", "worker", json!({ "prompt": prompt }));
    ScriptedProvider::new([calls(vec![call]), text("lead done")])
}

/// X-3: the worker runs one turn on the prompt, answers with its response
/// and keeps the exchange in its history.
#[tokio::test]
async fn worker_answers_and_keeps_its_history() {
    let worker = ConversationRuntime::new(ScriptedProvider::new([text("done")]), Tools::new())
        .into_tool("worker", "Does one task.");
    let worker = Arc::new(worker);
    let mut lead = ConversationRuntime::new(
        lead_calling_once("do it"),
        Tools::new().with(Shared(worker.clone())),
    );

    let turn = lead.run_turn("go").await.unwrap();

    assert_eq!(turn.tool_calls[0].output.content, "done");
    assert!(!turn.tool_calls[0].output.is_error);
    let agent = worker.agent.lock().await;
    let texts: Vec<&str> = agent
        .history()
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["do it", "done"]);
}

/// X-4: two calls to one worker in one step never run at once, even with
/// `tool_concurrency(4)`.
#[tokio::test]
async fn calls_to_one_worker_never_overlap() {
    let nap = Nap::default();
    let worker_model = ScriptedProvider::new([
        calls(vec![tool_use("n1", "nap")]),
        text("one"),
        calls(vec![tool_use("n2", "nap")]),
        text("two"),
    ]);
    let worker = ConversationRuntime::new(worker_model, Tools::new().with(nap.clone()))
        .into_tool("worker", "Does one task.");
    let lead_model = ScriptedProvider::new([
        calls(vec![
            tool_use_with("c1", "worker", json!({ "prompt": "first" })),
            tool_use_with("c2", "worker", json!({ "prompt": "second" })),
        ]),
        text("lead done"),
    ]);
    let mut lead = ConversationRuntime::builder(lead_model, Tools::new().with(worker))
        .tool_concurrency(4)
        .build();

    let turn = lead.run_turn("go").await.unwrap();

    assert_eq!(nap.most_at_once.load(Ordering::SeqCst), 1);
    assert_eq!(nap.calls.load(Ordering::SeqCst), 2);
    let outputs: Vec<&str> = turn
        .tool_calls
        .iter()
        .map(|c| c.output.content.as_str())
        .collect();
    assert_eq!(outputs, ["one", "two"]);
}

/// X-5: a worker whose turn fails answers with the error's text, marked as
/// failed; the lead's turn completes.
#[tokio::test]
async fn failing_worker_is_an_error_result() {
    let cut_short = vec![Ok(StreamEvent::ContentDelta("par".into()))];
    let worker =
        ConversationRuntime::new(ScriptedProvider::with_results([cut_short]), Tools::new())
            .into_tool("worker", "Does one task.");
    let mut lead = ConversationRuntime::new(lead_calling_once("do it"), Tools::new().with(worker));

    let turn = lead.run_turn("go").await.unwrap();

    let expected = RuntimeError::Provider(Error::UnexpectedEnd).to_string();
    assert_eq!(
        turn.tool_calls[0].output,
        ToolOutput {
            content: expected,
            is_error: true
        }
    );
    assert_eq!(turn.response, "lead done");
}

/// The input schema asks for one `prompt` string.
#[test]
fn definition_asks_for_a_prompt() {
    let worker = ConversationRuntime::new(ScriptedProvider::default(), Tools::new())
        .into_tool("worker", "Does one task.");

    let definition = worker.definition();

    assert_eq!(
        (definition.name.as_str(), definition.description.as_str()),
        ("worker", "Does one task.")
    );
    assert_eq!(definition.input_schema["required"], json!(["prompt"]));
    assert_eq!(
        definition.input_schema["properties"]["prompt"]["type"],
        "string"
    );
}

// --- Tools ---

/// A shared [`AgentTool`], so the test can read the worker after the lead
/// has run.
struct Shared<P, T>(Arc<AgentTool<P, T>>);

#[async_trait]
impl<P: LlmProvider, T: Toolbox> Tool for Shared<P, T> {
    fn definition(&self) -> ToolDefinition {
        self.0.definition()
    }

    async fn call(&self, input: serde_json::Value, transcript: &Transcript<'_>) -> ToolOutput {
        self.0.call(input, transcript).await
    }
}

/// The tool `nap`: sleeps, counting its calls and how many ran at once.
#[derive(Clone, Default)]
struct Nap {
    running: Arc<AtomicUsize>,
    most_at_once: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for Nap {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "nap".into(),
            description: "sleeps".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_at_once.fetch_max(now, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(30)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        ToolOutput {
            content: "slept".into(),
            is_error: false,
        }
    }
}
