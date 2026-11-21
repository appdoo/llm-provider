//! Tests for `AgentTool`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::Notify;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::agent::test_support::{calls, scripted, text, tool_use, tool_use_with};
use crate::agent::{RuntimeError, Tools, TurnSummary};
use crate::provider::test_stub::{StubBody, StubReply, StubServer};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Content, Error, OpenAi, StreamEvent};

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
    let worker = ConversationRuntime::new(
        scripted(ScriptedProvider::new([text("done")])),
        Tools::new(),
    )
    .into_tool("worker", "Does one task.");
    let worker = Arc::new(worker);
    let mut lead = ConversationRuntime::new(
        scripted(lead_calling_once("do it")),
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
    let worker = ConversationRuntime::new(scripted(worker_model), Tools::new().with(nap.clone()))
        .into_tool("worker", "Does one task.");
    let lead_model = ScriptedProvider::new([
        calls(vec![
            tool_use_with("c1", "worker", json!({ "prompt": "first" })),
            tool_use_with("c2", "worker", json!({ "prompt": "second" })),
        ]),
        text("lead done"),
    ]);
    let mut lead = ConversationRuntime::builder(scripted(lead_model), Tools::new().with(worker))
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
    let worker = ConversationRuntime::new(
        scripted(ScriptedProvider::with_results([cut_short])),
        Tools::new(),
    )
    .into_tool("worker", "Does one task.");
    let mut lead = ConversationRuntime::new(
        scripted(lead_calling_once("do it")),
        Tools::new().with(worker),
    );

    let turn = lead.run_turn("go").await.unwrap();

    let expected = RuntimeError::Model {
        error: Error::UnexpectedEnd,
        turn: TurnSummary::default(),
    }
    .to_string();
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
    let worker = ConversationRuntime::new(scripted(ScriptedProvider::default()), Tools::new())
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

/// T-4: cancelling the lead while its worker streams rolls both back: the
/// lead returns `Interrupted` with an empty history, and the worker's
/// history is empty too.
#[tokio::test]
async fn cancelled_lead_rolls_back_its_worker() {
    let within = Duration::from_secs(5);
    let chunk = json!({ "choices": [{ "index": 0, "delta": { "content": "w" } }] });
    let reply = StubReply {
        status: 200,
        content_type: "text/event-stream",
        headers: Vec::new(),
        body: StubBody::Events {
            lines: vec![format!("data: {chunk}\n\n")],
        },
    };
    let stub =
        StubServer::start_replies(vec![(("POST", "/v1/chat/completions"), vec![reply])]).await;
    let delta = Arc::new(Notify::new());
    let signal = delta.clone();
    let worker = OpenAi::at(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .llm("m");
    let worker = ConversationRuntime::builder(worker, Tools::new())
        .on_delta(move |_| signal.notify_one())
        .build()
        .into_tool("worker", "Does one task.");
    let worker = Arc::new(worker);
    let mut lead = ConversationRuntime::new(
        scripted(lead_calling_once("do it")),
        Tools::new().with(Shared(worker.clone())),
    );
    let token = CancellationToken::new();

    let (result, ()) = timeout(within, async {
        tokio::join!(lead.run_turn_cancellable("go", &token), async {
            delta.notified().await;
            token.cancel();
        })
    })
    .await
    .expect("the lead ends on the cancel");

    assert!(
        matches!(result, Err(RuntimeError::Interrupted { .. })),
        "expected Interrupted, got {result:?}"
    );
    assert_eq!(lead.history(), []);
    let agent = timeout(within, worker.agent.lock())
        .await
        .expect("the worker's turn was dropped");
    assert_eq!(agent.history(), []);
}

// --- Tools ---

/// A shared [`AgentTool`], so the test can read the worker after the lead
/// has run.
struct Shared<T>(Arc<AgentTool<T>>);

#[async_trait]
impl<T: Toolbox> Tool for Shared<T> {
    fn definition(&self) -> ToolDefinition {
        self.0.definition()
    }

    async fn call(&self, input: serde_json::Value, context: &CallContext<'_>) -> ToolOutput {
        self.0.call(input, context).await
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

    async fn call(&self, _input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
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
