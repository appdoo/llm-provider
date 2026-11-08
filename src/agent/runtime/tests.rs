//! Tests for the turn loop, against `ScriptedProvider`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::agent::test_support::{calls, text, tool_use, tool_use_with};
use crate::agent::{Tool, Tools, Transcript};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{ApiError, ErrorKind, StreamEvent, ToolDefinition};

// --- Scripts ---

/// `(tool_use_id, content, is_error)` of every block; panics on a non-result block.
fn tool_results(content: &[Content]) -> Vec<(&str, &str, bool)> {
    content
        .iter()
        .map(|block| match block {
            Content::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => (tool_use_id.as_str(), content.as_str(), *is_error),
            other => panic!("expected ToolResult, got {other:?}"),
        })
        .collect()
}

// --- Tools ---

/// A tool called `name` that answers every call with `output`.
struct Fixed {
    name: &'static str,
    output: ToolOutput,
}

impl Fixed {
    fn ok(name: &'static str, content: &str) -> Self {
        Self {
            name,
            output: ToolOutput {
                content: content.into(),
                is_error: false,
            },
        }
    }

    fn failing(name: &'static str, content: &str) -> Self {
        Self {
            name,
            output: ToolOutput {
                content: content.into(),
                is_error: true,
            },
        }
    }
}

fn definition(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: format!("the {name} tool"),
        input_schema: json!({ "type": "object", "properties": {} }),
    }
}

#[async_trait]
impl Tool for Fixed {
    fn definition(&self) -> ToolDefinition {
        definition(self.name)
    }

    async fn call(&self, _input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        self.output.clone()
    }
}

/// The tool `boom`, which panics.
struct Panics;

#[async_trait]
impl Tool for Panics {
    fn definition(&self) -> ToolDefinition {
        definition("boom")
    }

    async fn call(&self, _input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        panic!("boom")
    }
}

/// The tool `slow`: sleeps `input.ms` milliseconds and records how many
/// calls run at once.
#[derive(Clone, Default)]
struct Slow {
    running: Arc<AtomicUsize>,
    most_at_once: Arc<AtomicUsize>,
}

impl Slow {
    fn most_at_once(&self) -> usize {
        self.most_at_once.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Tool for Slow {
    fn definition(&self) -> ToolDefinition {
        definition("slow")
    }

    async fn call(&self, input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_at_once.fetch_max(now, Ordering::SeqCst);

        let ms = input["ms"].as_u64().unwrap_or(20);
        tokio::time::sleep(Duration::from_millis(ms)).await;

        self.running.fetch_sub(1, Ordering::SeqCst);
        ToolOutput {
            content: format!("slept {ms}"),
            is_error: false,
        }
    }
}

/// Three calls to `slow`, the first sleeping longest.
fn three_slow_calls() -> Vec<StreamEvent> {
    calls(vec![
        tool_use_with("t-1", "slow", json!({ "ms": 60 })),
        tool_use_with("t-2", "slow", json!({ "ms": 30 })),
        tool_use_with("t-3", "slow", json!({ "ms": 10 })),
    ])
}

/// Runs the three slow calls with `concurrency` and returns the most that
/// ran at once and the runtime.
async fn run_three_slow(concurrency: usize) -> (usize, Vec<Message>) {
    let slow = Slow::default();
    let provider = ScriptedProvider::new([three_slow_calls(), text("done")]);
    let mut runtime = ConversationRuntime::builder(provider, Tools::new().with(slow.clone()))
        .tool_concurrency(concurrency)
        .build();

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!(turn.tool_calls.len(), 3);
    (slow.most_at_once(), runtime.messages)
}

// --- One turn ---

/// A reply without tool calls is the turn's response, in one model call.
#[tokio::test]
async fn run_turn_returns_response() {
    let mut runtime =
        ConversationRuntime::new(ScriptedProvider::new([text("Hello, world!")]), Tools::new());

    let turn = runtime.run_turn("hi").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "Hello, world!".into(),
            tool_calls: vec![],
            iterations: 1
        }
    );
}

/// A tool call runs, its result goes back, and the next reply ends the turn.
#[tokio::test]
async fn run_turn_with_tool_call() {
    let provider = ScriptedProvider::new([
        calls(vec![tool_use("t-1", "echo")]),
        text("done after tool"),
    ]);
    let mut runtime =
        ConversationRuntime::new(provider, Tools::new().with(Fixed::ok("echo", "hello")));

    let turn = runtime.run_turn("use a tool").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "done after tool".into(),
            tool_calls: vec![ToolCall {
                name: "echo".into(),
                input: json!({}),
                output: ToolOutput {
                    content: "hello".into(),
                    is_error: false,
                },
            }],
            iterations: 2
        }
    );
}

/// A turn with tools but no tool call makes one model call.
#[tokio::test]
async fn run_turn_no_tools_single_iteration() {
    let tools = Tools::new().with(Fixed::ok("echo", "unused"));
    let mut runtime = ConversationRuntime::new(ScriptedProvider::new([text("just text")]), tools);

    let turn = runtime.run_turn("hello").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "just text".into(),
            tool_calls: vec![],
            iterations: 1
        }
    );
}

/// The system prompt goes with the request.
#[tokio::test]
async fn run_turn_passes_system_prompt() {
    let provider = ScriptedProvider::new([text("ok")]);
    let mut runtime = ConversationRuntime::builder(provider.clone(), Tools::new())
        .system("You are a helpful assistant.")
        .build();

    runtime.run_turn("hi").await.unwrap();

    let requests = provider.requests();
    assert_eq!(
        requests[0].system.as_deref(),
        Some("You are a helpful assistant.")
    );
}

/// The deltas reach `on_delta` in order.
#[tokio::test]
async fn run_turn_on_delta_receives_text() {
    let deltas = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&deltas);
    let script = ["Hello", ", ", "world!"].map(|t| StreamEvent::ContentDelta(t.into()));
    let provider = ScriptedProvider::new([script
        .into_iter()
        .chain([StreamEvent::MessageStop])
        .collect()]);
    let mut runtime = ConversationRuntime::builder(provider, Tools::new())
        .on_delta(move |text| recorder.lock().unwrap().push(text.to_string()))
        .build();

    let turn = runtime.run_turn("hi").await.unwrap();

    assert_eq!(turn.response, "Hello, world!");
    assert_eq!(*deltas.lock().unwrap(), ["Hello", ", ", "world!"]);
}

/// B-6: `on_delta` records `"a"`, `"b"`; the response is `"ab"`.
#[tokio::test]
async fn on_delta_records_each_delta() {
    let deltas = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&deltas);
    let script = vec![
        StreamEvent::ContentDelta("a".into()),
        StreamEvent::ContentDelta("b".into()),
        StreamEvent::MessageStop,
    ];
    let mut runtime = ConversationRuntime::builder(ScriptedProvider::new([script]), Tools::new())
        .on_delta(move |text| recorder.lock().unwrap().push(text.to_string()))
        .build();

    let turn = runtime.run_turn("x").await.unwrap();

    assert_eq!(*deltas.lock().unwrap(), ["a", "b"]);
    assert_eq!(turn.response, "ab");
}

/// A mid-stream `Api` error "boom" surfaces unchanged as `Error::Api`.
#[tokio::test]
async fn stream_error_maps_to_stream_variant_not_auth() {
    let script = vec![
        Ok(StreamEvent::ContentDelta("partial".into())),
        Err(Error::Api(ApiError::new(ErrorKind::Overloaded, "boom"))),
    ];
    let mut runtime =
        ConversationRuntime::new(ScriptedProvider::with_results([script]), Tools::new());

    match runtime.run_turn("go").await {
        Err(RuntimeError::Provider(Error::Api(e))) => assert_eq!(e.message(), "boom"),
        other => panic!("expected Provider(Api(\"boom\")), got {other:?}"),
    }
}

/// B-3: a reply cut at `max_tokens` is `Truncated` with the partial text;
/// only the user message stays in the history.
#[tokio::test]
async fn max_tokens_stop_is_truncated() {
    let script = vec![
        StreamEvent::ContentDelta("partial".into()),
        StreamEvent::Stop(StopReason::MaxTokens),
        StreamEvent::MessageStop,
    ];
    let mut runtime = ConversationRuntime::new(ScriptedProvider::new([script]), Tools::new());

    match runtime.run_turn("go").await {
        Err(RuntimeError::Truncated { partial_text }) => assert_eq!(partial_text, "partial"),
        other => panic!("expected Truncated, got {other:?}"),
    }
    assert_eq!(runtime.messages.len(), 1, "only the user message is kept");
}

/// Any other stop reason ends the turn normally.
#[tokio::test]
async fn end_turn_stop_is_ok() {
    let script = vec![
        StreamEvent::ContentDelta("done".into()),
        StreamEvent::Stop(StopReason::EndTurn),
        StreamEvent::MessageStop,
    ];
    let mut runtime = ConversationRuntime::new(ScriptedProvider::new([script]), Tools::new());

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "done".into(),
            tool_calls: vec![],
            iterations: 1
        }
    );
}

// --- Iterations ---

/// A model that asks for `echo` on every call, `n` times.
fn always_echo(n: usize) -> ScriptedProvider {
    ScriptedProvider::new((0..n).map(|i| calls(vec![tool_use(&format!("t-{i}"), "echo")])))
}

/// With `max_iterations(1)` a second model call is refused.
#[tokio::test]
async fn run_turn_max_iterations_exceeded() {
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = ConversationRuntime::builder(always_echo(1), tools)
        .max_iterations(1)
        .build();

    match runtime.run_turn("infinite").await {
        Err(RuntimeError::MaxIterations(n)) => assert_eq!(n, 1),
        other => panic!("expected MaxIterations, got {other:?}"),
    }
}

/// `max_iterations(5)` drives the loop: `MaxIterations(5)`.
#[tokio::test]
async fn custom_max_iterations_propagates_to_tool_loop() {
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = ConversationRuntime::builder(always_echo(5), tools)
        .max_iterations(5)
        .build();

    match runtime.run_turn("go").await {
        Err(RuntimeError::MaxIterations(n)) => assert_eq!(n, 5),
        other => panic!("expected MaxIterations(5), got {other:?}"),
    }
}

/// B-4: `max_iterations(3)` stops after 3 model calls with the iteration error.
#[tokio::test]
async fn max_iterations_stops_after_three_model_calls() {
    let provider = always_echo(3);
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = ConversationRuntime::builder(provider.clone(), tools)
        .max_iterations(3)
        .build();

    let result = runtime.run_turn("go").await;

    assert!(
        matches!(result, Err(RuntimeError::MaxIterations(3))),
        "got {result:?}"
    );
    assert_eq!(provider.requests().len(), 3);
}

/// A 20-call tool loop: 19 tool rounds, then text.
#[tokio::test]
async fn twenty_turn_tool_loop() {
    let rounds = (0..19).map(|i| calls(vec![tool_use(&format!("t-{i}"), "echo")]));
    let provider = ScriptedProvider::new(rounds.chain([text("done")]));
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = ConversationRuntime::new(provider.clone(), tools);

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!((turn.iterations, turn.tool_calls.len()), (20, 19));
    assert_eq!(provider.requests().len(), 20);
}

// --- Settings ---

/// B-5: `new` uses 8192 / 25 / 4 / 100 000 and no system prompt.
#[test]
fn new_uses_the_defaults() {
    let runtime = ConversationRuntime::new(ScriptedProvider::default(), Tools::new());

    let s = &runtime.settings;
    assert_eq!(
        (
            s.max_tokens,
            s.max_iterations,
            s.tool_concurrency,
            s.max_tool_output_chars
        ),
        (8192, 25, 4, 100_000)
    );
    assert_eq!(s.system, None);
}

/// The builder's setters reach the runtime.
#[test]
fn builder_settings_reach_the_runtime() {
    let runtime = ConversationRuntime::builder(ScriptedProvider::default(), Tools::new())
        .max_iterations(7)
        .max_tokens(256)
        .tool_concurrency(2)
        .max_tool_output_chars(10)
        .build();

    let s = &runtime.settings;
    assert_eq!(
        (
            s.max_tokens,
            s.max_iterations,
            s.tool_concurrency,
            s.max_tool_output_chars
        ),
        (256, 7, 2, 10)
    );
}

// --- Tool calls ---

/// B-2: with `tool_concurrency(2)` never more than 2 run at once; the
/// results come back in call order.
#[tokio::test]
async fn tool_calls_run_at_most_n_at_once_in_call_order() {
    let (most_at_once, messages) = run_three_slow(2).await;

    assert_eq!(most_at_once, 2);
    assert_eq!(
        tool_results(&messages[2].content),
        [
            ("t-1", "slept 60", false),
            ("t-2", "slept 30", false),
            ("t-3", "slept 10", false)
        ]
    );
}

/// With concurrency 4 the three calls run at once.
#[tokio::test]
async fn test_parallel_tool_execution() {
    let (most_at_once, _) = run_three_slow(4).await;

    assert_eq!(most_at_once, 3);
}

/// With concurrency 1 the calls run one after another.
#[tokio::test]
async fn test_sequential_with_concurrency_1() {
    let (most_at_once, _) = run_three_slow(1).await;

    assert_eq!(most_at_once, 1);
}

/// One failing tool does not stop its siblings; every result comes back.
#[tokio::test]
async fn test_parallel_error_isolation() {
    let first = calls(vec![
        tool_use("t-1", "good"),
        tool_use("t-2", "bad"),
        tool_use("t-3", "good"),
    ]);
    let tools = Tools::new()
        .with(Fixed::ok("good", "ok: good"))
        .with(Fixed::failing("bad", "tool failed"));
    let mut runtime = ConversationRuntime::new(ScriptedProvider::new([first, text("done")]), tools);

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!((turn.tool_calls.len(), turn.response.as_str()), (3, "done"));
    assert_eq!(
        tool_results(&runtime.messages[2].content),
        [
            ("t-1", "ok: good", false),
            ("t-2", "tool failed", true),
            ("t-3", "ok: good", false)
        ]
    );
}

/// The results follow the order of the calls, not of their completion.
#[tokio::test]
async fn test_parallel_order_preservation() {
    let (_, messages) = run_three_slow(4).await;

    let ids: Vec<&str> = tool_results(&messages[2].content)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(ids, ["t-1", "t-2", "t-3"]);
}

/// A panicking tool becomes an error result; its sibling still runs.
#[tokio::test]
async fn panicking_tool_becomes_error_result() {
    let first = calls(vec![tool_use("t-boom", "boom"), tool_use("t-ok", "ok")]);
    let tools = Tools::new().with(Panics).with(Fixed::ok("ok", "fine"));
    let mut runtime = ConversationRuntime::new(ScriptedProvider::new([first, text("done")]), tools);

    runtime.run_turn("go").await.unwrap();

    let results = &runtime.messages[2];
    assert_eq!(results.role, MessageRole::User);
    assert_eq!(
        tool_results(&results.content),
        [
            ("t-boom", "tool boom panicked", true),
            ("t-ok", "fine", false)
        ]
    );
}

/// Output over `max_tool_output_chars` keeps head and tail.
#[tokio::test]
async fn tool_output_is_capped() {
    let big = "x".repeat(300_000);
    let first = calls(vec![tool_use("t-big", "big")]);
    let tools = Tools::new().with(Fixed::ok("big", &big));
    let mut runtime =
        ConversationRuntime::builder(ScriptedProvider::new([first, text("done")]), tools)
            .max_tool_output_chars(100_000)
            .build();

    runtime.run_turn("go").await.unwrap();

    let expected = "x".repeat(50_000) + "\n[… 200000 characters omitted …]\n" + &"x".repeat(50_000);
    assert_eq!(
        tool_results(&runtime.messages[2].content),
        [("t-big", expected.as_str(), false)]
    );
}

// --- The caller's program ---

/// B-7: the caller's program from Spec 36, with `ScriptedProvider` in place
/// of the model: the edit lands in `docs/README.md`.
#[cfg(feature = "fs-tools")]
#[tokio::test]
async fn callers_program_edits_the_file() {
    use crate::fs_tools::{Files, Sandbox};

    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs");
    std::fs::create_dir(&docs).unwrap();
    std::fs::write(docs.join("README.md"), "Teh crate.\n").unwrap();
    let edit = json!({ "path": "README.md", "old_string": "Teh", "new_string": "The" });
    let model = ScriptedProvider::new([
        calls(vec![tool_use_with(
            "c1",
            "read",
            json!({ "path": "README.md" }),
        )]),
        calls(vec![tool_use_with("c2", "edit", edit)]),
        text("Fixed one typo."),
    ]);

    let sandbox = Sandbox::new(&docs).unwrap();
    let tools = Tools::new()
        .with_all(Files::new(sandbox))
        .only(["read", "edit"]);

    let mut agent = ConversationRuntime::builder(model, tools)
        .system("You fix typos in the files you are given.")
        .max_iterations(10)
        .on_delta(|text| print!("{text}"))
        .build();

    let turn = agent.run_turn("Fix the typos in README.md.").await.unwrap();
    assert_eq!(turn.tool_calls.len(), 2);
    assert_eq!(
        std::fs::read_to_string(docs.join("README.md")).unwrap(),
        "The crate.\n"
    );
}

/// `run_turn` can run on a spawned task.
#[test]
fn run_turn_future_is_send() {
    fn assert_send<F: Send>(_: &F) {}
    let mut runtime = ConversationRuntime::new(ScriptedProvider::default(), Tools::new());
    let future = runtime.run_turn("go");
    assert_send(&future);
}
