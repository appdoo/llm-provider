//! Tests for the turn loop, against `ScriptedProvider`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::{mpsc, Notify};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::agent::test_support::{calls, scripted, text, text_and_calls, tool_use, tool_use_with};
use crate::agent::{Advisor, CallContext, Tool, Tools};
use crate::provider::test_stub::{StubBody, StubReply, StubServer};
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{
    Anthropic, ApiError, CacheTtl, ErrorKind, OpenAi, RetryPolicy, StreamEvent, ToolDefinition,
};

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
pub(super) struct Fixed {
    name: &'static str,
    output: ToolOutput,
}

impl Fixed {
    pub(super) fn ok(name: &'static str, content: &str) -> Self {
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

    async fn call(&self, _input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
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

    async fn call(&self, _input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
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

    async fn call(&self, input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
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
    let mut runtime = Session::builder(scripted(provider), Tools::new().with(slow.clone()))
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
    let mut runtime = Session::new(
        scripted(ScriptedProvider::new([text("Hello, world!")])),
        Tools::new(),
    );

    let turn = runtime.run_turn("hi").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "Hello, world!".into(),
            tool_calls: vec![],
            iterations: 1,
            ..TurnSummary::default()
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
    let mut runtime = Session::new(
        scripted(provider),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );

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
            iterations: 2,
            ..TurnSummary::default()
        }
    );
}

/// A-1: the text of a reply that calls a tool is kept beside the call, text
/// first, in the assistant message the next request carries.
#[tokio::test]
async fn reply_text_is_kept_beside_its_tool_calls() {
    let provider = ScriptedProvider::new([
        text_and_calls("PLAN: call echo.", vec![tool_use("t-1", "echo")]),
        text("done"),
    ]);
    let mut runtime = Session::new(
        scripted(provider.clone()),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );

    runtime.run_turn("go").await.unwrap();

    let requests = provider.requests();
    let sent = &requests[1].messages;
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0], Message::user("go"));
    assert_eq!(
        sent[1],
        Message {
            role: MessageRole::Assistant,
            content: vec![
                Content::Text {
                    text: "PLAN: call echo.".into()
                },
                Content::ToolUse {
                    id: "t-1".into(),
                    name: "echo".into(),
                    input: json!({}),
                },
            ],
        }
    );
    assert_eq!(sent[2].role, MessageRole::User);
    assert_eq!(tool_results(&sent[2].content), [("t-1", "hello", false)]);
    assert_eq!(runtime.history().last(), Some(&Message::assistant("done")));
}

/// A-2: a reply of tool calls alone gives an assistant message of exactly
/// one block, the `ToolUse`.
#[tokio::test]
async fn reply_without_text_stores_only_its_tool_calls() {
    let provider = ScriptedProvider::new([calls(vec![tool_use("t-1", "echo")]), text("done")]);
    let mut runtime = Session::new(
        scripted(provider),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );

    runtime.run_turn("go").await.unwrap();

    assert_eq!(
        runtime.history()[1].content,
        [Content::ToolUse {
            id: "t-1".into(),
            name: "echo".into(),
            input: json!({}),
        }]
    );
}

/// A-3: an empty reply ends the turn with an empty response and stores no
/// assistant message.
#[tokio::test]
async fn empty_reply_stores_no_message() {
    let provider = ScriptedProvider::new([vec![StreamEvent::MessageStop]]);
    let mut runtime = Session::new(scripted(provider), Tools::new());

    let turn = runtime.run_turn("hi").await.unwrap();

    assert_eq!(turn.response, "");
    assert_eq!(runtime.history(), [Message::user("hi")]);
}

/// A turn with tools but no tool call makes one model call.
#[tokio::test]
async fn run_turn_no_tools_single_iteration() {
    let tools = Tools::new().with(Fixed::ok("echo", "unused"));
    let mut runtime = Session::new(scripted(ScriptedProvider::new([text("just text")])), tools);

    let turn = runtime.run_turn("hello").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "just text".into(),
            tool_calls: vec![],
            iterations: 1,
            ..TurnSummary::default()
        }
    );
}

/// The system prompt goes with the request.
#[tokio::test]
async fn run_turn_passes_system_prompt() {
    let provider = ScriptedProvider::new([text("ok")]);
    let mut runtime = Session::builder(scripted(provider.clone()), Tools::new())
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
    let mut runtime = Session::builder(scripted(provider), Tools::new())
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
    let mut runtime = Session::builder(scripted(ScriptedProvider::new([script])), Tools::new())
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
    let mut runtime = Session::new(
        scripted(ScriptedProvider::with_results([script])),
        Tools::new(),
    );

    match runtime.run_turn("go").await {
        Err(SessionError::Model {
            error: Error::Api(e),
            ..
        }) => assert_eq!(e.message(), "boom"),
        other => panic!("expected Model(Api(\"boom\")), got {other:?}"),
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
    let mut runtime = Session::new(scripted(ScriptedProvider::new([script])), Tools::new());

    match runtime.run_turn("go").await {
        Err(error @ SessionError::Truncated { .. }) => {
            assert_eq!(error.turn().response, "partial");
        }
        other => panic!("expected Truncated, got {other:?}"),
    }
    assert_eq!(runtime.messages.len(), 0, "the failed turn leaves no trace");
}

/// Any other stop reason ends the turn normally.
#[tokio::test]
async fn end_turn_stop_is_ok() {
    let script = vec![
        StreamEvent::ContentDelta("done".into()),
        StreamEvent::Stop(StopReason::EndTurn),
        StreamEvent::MessageStop,
    ];
    let mut runtime = Session::new(scripted(ScriptedProvider::new([script])), Tools::new());

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!(
        turn,
        TurnSummary {
            response: "done".into(),
            tool_calls: vec![],
            iterations: 1,
            ..TurnSummary::default()
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
    let mut runtime = Session::builder(scripted(always_echo(1)), tools)
        .max_iterations(1)
        .build();

    match runtime.run_turn("infinite").await {
        Err(SessionError::MaxIterations { limit, .. }) => assert_eq!(limit, 1),
        other => panic!("expected MaxIterations, got {other:?}"),
    }
}

/// `max_iterations(5)` drives the loop: `MaxIterations(5)`.
#[tokio::test]
async fn custom_max_iterations_propagates_to_tool_loop() {
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = Session::builder(scripted(always_echo(5)), tools)
        .max_iterations(5)
        .build();

    match runtime.run_turn("go").await {
        Err(SessionError::MaxIterations { limit, .. }) => assert_eq!(limit, 5),
        other => panic!("expected MaxIterations(5), got {other:?}"),
    }
}

/// B-4: `max_iterations(3)` stops after 3 model calls with the iteration error.
#[tokio::test]
async fn max_iterations_stops_after_three_model_calls() {
    let provider = always_echo(3);
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = Session::builder(scripted(provider.clone()), tools)
        .max_iterations(3)
        .build();

    let result = runtime.run_turn("go").await;

    assert!(
        matches!(result, Err(SessionError::MaxIterations { limit: 3, .. })),
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
    let mut runtime = Session::new(scripted(provider.clone()), tools);

    let turn = runtime.run_turn("go").await.unwrap();

    assert_eq!((turn.iterations, turn.tool_calls.len()), (20, 19));
    assert_eq!(provider.requests().len(), 20);
}

// --- Settings ---

/// B-5: `new` uses 8192 / 25 / 4 / 100 000 and no system prompt.
#[test]
fn new_uses_the_defaults() {
    let runtime = Session::new(scripted(ScriptedProvider::default()), Tools::new());

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
    let runtime = Session::builder(scripted(ScriptedProvider::default()), Tools::new())
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
    let mut runtime = Session::new(
        scripted(ScriptedProvider::new([first, text("done")])),
        tools,
    );

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
    let mut runtime = Session::new(
        scripted(ScriptedProvider::new([first, text("done")])),
        tools,
    );

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

/// X-6: `tool_calls` records each call of the turn, in call order, with its
/// input and output.
#[tokio::test]
async fn turn_records_every_tool_call_in_order() {
    let first = calls(vec![
        tool_use_with("t-a", "a", json!({ "n": 1 })),
        tool_use_with("t-b", "b", json!({ "n": 2 })),
    ]);
    let tools = Tools::new()
        .with(Fixed::ok("a", "out a"))
        .with(Fixed::failing("b", "out b"));
    let mut runtime = Session::new(
        scripted(ScriptedProvider::new([first, text("done")])),
        tools,
    );

    let turn = runtime.run_turn("go").await.unwrap();

    let record = |name: &str, input, content: &str, is_error| ToolCall {
        name: name.into(),
        input,
        output: ToolOutput {
            content: content.into(),
            is_error,
        },
    };
    assert_eq!(
        turn.tool_calls,
        [
            record("a", json!({ "n": 1 }), "out a", false),
            record("b", json!({ "n": 2 }), "out b", true)
        ]
    );
    assert_eq!(runtime.history(), runtime.messages.as_slice());
}

/// Output over `max_tool_output_chars` keeps head and tail.
#[tokio::test]
async fn tool_output_is_capped() {
    let big = "x".repeat(300_000);
    let first = calls(vec![tool_use("t-big", "big")]);
    let tools = Tools::new().with(Fixed::ok("big", &big));
    let mut runtime = Session::builder(
        scripted(ScriptedProvider::new([first, text("done")])),
        tools,
    )
    .max_tool_output_chars(100_000)
    .build();

    runtime.run_turn("go").await.unwrap();

    let omitted = "\n[… 200000 characters omitted …]\n";
    let expected = "x".repeat(50_000) + omitted + &"x".repeat(50_000);
    assert_eq!(
        tool_results(&runtime.messages[2].content),
        [("t-big", expected.as_str(), false)]
    );
}

// --- The caller's program ---

/// The caller program of the file-editing agent, with `ScriptedProvider` in place
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

    let mut agent = Session::builder(scripted(model), tools)
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
    let mut runtime = Session::new(scripted(ScriptedProvider::default()), Tools::new());
    let future = runtime.run_turn("go");
    assert_send(&future);
}

// --- Cancel ---

/// How long a cancel test waits for the turn, a signal or a channel.
const WITHIN: Duration = Duration::from_secs(5);

/// One OpenAI chat chunk carrying `text`, as an event-stream line.
fn chat_chunk(text: &str) -> String {
    let chunk = json!({ "choices": [{ "index": 0, "delta": { "content": text } }] });
    format!("data: {chunk}\n\n")
}

/// A reply that streams the chunk `text`, then holds the connection open.
fn holding(text: &str) -> StubReply {
    StubReply {
        status: 200,
        content_type: "text/event-stream",
        headers: Vec::new(),
        body: StubBody::Events {
            lines: vec![chat_chunk(text)],
        },
    }
}

/// A complete reply that calls the tool `name` once, as call `id`.
fn calling(id: &str, name: &str) -> StubReply {
    let call = json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{
        "index": 0, "id": id, "type": "function",
        "function": { "name": name, "arguments": "{}" }
    }] }, "finish_reason": null }] });
    let stop = json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] });
    StubReply::full(
        200,
        "text/event-stream",
        &format!("data: {call}\n\ndata: {stop}\n\ndata: [DONE]\n\n"),
    )
}

/// A complete reply that answers `text`.
fn answering(text: &str) -> StubReply {
    let stop = json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] });
    StubReply::full(
        200,
        "text/event-stream",
        &format!("{}data: {stop}\n\ndata: [DONE]\n\n", chat_chunk(text)),
    )
}

/// A stub that answers the chat requests with `replies`, in order.
async fn chat_stub(replies: Vec<StubReply>) -> StubServer {
    StubServer::start_replies(vec![(("POST", "/v1/chat/completions"), replies)]).await
}

/// The model `m` on the stub's `/v1` root.
fn stub_model(stub: &StubServer) -> Llm {
    OpenAi::at(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .llm("m")
}

/// The partial text and tool calls of an `Interrupted` error.
fn interrupted(result: Result<TurnSummary, SessionError>) -> (String, Vec<ToolCall>) {
    match result {
        Err(SessionError::Interrupted { turn }) => (turn.response, turn.tool_calls),
        other => panic!("expected Interrupted, got {other:?}"),
    }
}

/// The tool `wait`: hands a clone of the turn's token to a spawned task
/// that sends `stopped` once it fires, signals `started`, then never
/// returns.
struct Wait {
    started: Arc<Notify>,
    stopped: mpsc::Sender<&'static str>,
}

#[async_trait]
impl Tool for Wait {
    fn definition(&self) -> ToolDefinition {
        definition("wait")
    }

    async fn call(&self, _input: serde_json::Value, context: &CallContext<'_>) -> ToolOutput {
        let cancel = context.cancel.clone();
        let stopped = self.stopped.clone();
        tokio::spawn(async move {
            cancel.cancelled().await;
            stopped.send("stopped").await.expect("the test listens");
        });

        self.started.notify_one();
        std::future::pending().await
    }
}

/// An [`Advisor`] that records the turn number of every call and, in turn
/// 1, cancels `token` once the advisor has answered.
struct CountedAdvisor {
    advisor: Advisor,
    turns: Arc<Mutex<Vec<u64>>>,
    token: CancellationToken,
}

#[async_trait]
impl Tool for CountedAdvisor {
    fn definition(&self) -> ToolDefinition {
        self.advisor.definition()
    }

    async fn call(&self, input: serde_json::Value, context: &CallContext<'_>) -> ToolOutput {
        self.turns.lock().unwrap().push(context.turn);
        let output = self.advisor.call(input, context).await;
        if context.turn == 1 {
            self.token.cancel();
        }
        output
    }
}

/// T-1: a token cancelled on the first delta returns `Interrupted` with
/// the text so far and no tool calls; the history is empty again.
#[tokio::test]
async fn cancelled_turn_returns_its_text_and_rolls_back() {
    let stub = chat_stub(vec![holding("hi there")]).await;
    let token = CancellationToken::new();
    let canceller = token.clone();
    let mut runtime = Session::builder(stub_model(&stub), Tools::new())
        .on_delta(move |_| canceller.cancel())
        .build();

    let result = timeout(WITHIN, runtime.run_turn_cancellable("hi", &token))
        .await
        .expect("the turn ends on the cancel");

    let (partial_text, tool_calls) = interrupted(result);
    assert_eq!(partial_text, "hi there");
    assert_eq!(tool_calls, []);
    assert_eq!(runtime.history(), []);
}

/// H1: the same test pins `Interrupted` and the two-message history.
/// T-2: a cancel during a tool call reaches the tool's spawned task through
/// `CallContext::cancel`; the turn returns the text and the completed call,
/// and the history is the earlier turn's two messages.
#[tokio::test]
async fn cancel_reaches_tools_and_keeps_completed_calls() {
    let provider = ScriptedProvider::new([
        text("first"),
        text_and_calls("looking", vec![tool_use("n1", "note")]),
        calls(vec![tool_use("w1", "wait")]),
    ]);
    let started = Arc::new(Notify::new());
    let (stopped, mut stops) = mpsc::channel(1);
    let tools = Tools::new().with(Fixed::ok("note", "noted")).with(Wait {
        started: started.clone(),
        stopped,
    });
    let mut runtime = Session::new(scripted(provider), tools);
    runtime.run_turn("one").await.expect("the first turn");
    assert_eq!(runtime.history().len(), 2);
    let token = CancellationToken::new();

    let (result, ()) = timeout(WITHIN, async {
        tokio::join!(runtime.run_turn_cancellable("two", &token), async {
            started.notified().await;
            token.cancel();
        })
    })
    .await
    .expect("the turn ends on the cancel");

    let (partial_text, tool_calls) = interrupted(result);
    assert_eq!(partial_text, "looking");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, "note");
    let stop = timeout(WITHIN, stops.recv()).await.expect("the task stops");
    assert_eq!(stop, Some("stopped"));
    assert_eq!(runtime.history().len(), 2);
}

/// I1: a cancel while a tool waits returns `Interrupted` carrying the turn:
/// no finished tool call, one iteration, the model call's usage; the history is
/// unchanged.
#[tokio::test]
async fn interrupted_carries_the_turn_so_far() {
    let wait_call = json!({"type":"content_block_start","index":0,"content_block":
        {"type":"tool_use","id":"w-1","name":"wait","input":{}}});
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![anthropic_counted(
            &[wait_call, json!({"type":"content_block_stop","index":0})],
            "tool_use",
            (100, 5, 0),
        )],
    )])
    .await;
    let model = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x");
    let started = Arc::new(Notify::new());
    let (stopped, _stops) = mpsc::channel(1);
    let tools = Tools::new().with(Wait {
        started: started.clone(),
        stopped,
    });
    let mut runtime = Session::new(model, tools);
    let token = CancellationToken::new();

    let (result, ()) = timeout(WITHIN, async {
        tokio::join!(runtime.run_turn_cancellable("go", &token), async {
            started.notified().await;
            token.cancel();
        })
    })
    .await
    .expect("the turn ends on the cancel");

    let error = result.expect_err("the cancel");
    assert!(
        matches!(error, SessionError::Interrupted { .. }),
        "{error:?}"
    );
    let turn = error.turn();
    assert_eq!(turn.tool_calls.len(), 0);
    assert_eq!(turn.iterations, 1);
    assert_eq!(turn.usage.input_tokens, 100);
    assert_eq!(runtime.history(), []);
}

/// T-3: a `run_turn` future dropped on its first delta leaves the history
/// empty.
#[tokio::test]
async fn dropped_turn_rolls_back() {
    let stub = chat_stub(vec![holding("hi")]).await;
    let delta = Arc::new(Notify::new());
    let signal = delta.clone();
    let mut runtime = Session::builder(stub_model(&stub), Tools::new())
        .on_delta(move |_| signal.notify_one())
        .build();

    let dropped = timeout(WITHIN, async {
        tokio::select! {
            biased;
            result = runtime.run_turn("hi") => panic!("the turn ended: {result:?}"),
            () = delta.notified() => {}
        }
    })
    .await;

    assert!(dropped.is_ok(), "the first delta arrives");
    assert_eq!(runtime.history(), []);
}

/// H2: a turn dropped after its request started leaves the two
/// messages of the earlier turn.
#[tokio::test]
async fn dropped_turn_keeps_the_earlier_history() {
    let stub = chat_stub(vec![answering("first"), holding("hi")]).await;
    let delta = Arc::new(Notify::new());
    let signal = delta.clone();
    let mut runtime = Session::builder(stub_model(&stub), Tools::new())
        .on_delta(move |_| signal.notify_one())
        .build();
    runtime.run_turn("one").await.expect("the first turn");
    assert_eq!(runtime.history().len(), 2);

    let dropped = timeout(WITHIN, async {
        tokio::select! {
            biased;
            result = runtime.run_turn("two") => panic!("the turn ended: {result:?}"),
            () = delta.notified() => {}
        }
    })
    .await;

    assert!(dropped.is_ok(), "the delta arrives");
    assert_eq!(runtime.history().len(), 2);
}

/// H3: a reply without a tool call adds the user message and the
/// reply to the two messages already there.
#[tokio::test]
async fn answered_turn_appends_two_messages() {
    let provider = ScriptedProvider::new([text("first"), text("ok")]);
    let mut runtime = Session::new(scripted(provider), Tools::new());
    runtime.run_turn("one").await.expect("the first turn");
    assert_eq!(runtime.history().len(), 2);

    runtime.run_turn("hi").await.expect("the second turn");

    assert_eq!(runtime.history().len(), 4);
    assert_eq!(runtime.history()[3], Message::assistant("ok"));
}

/// H4: a model error (500, no retry) leaves the two messages of the
/// earlier turn; the failed prompt is not kept.
#[tokio::test]
async fn failed_turn_leaves_the_history_unchanged() {
    let stub = chat_stub(vec![
        answering("first"),
        StubReply::full(500, "application/json", r#"{"error":{"message":"down"}}"#),
    ])
    .await;
    let model = OpenAi::builder(format!("{}/v1", stub.url()))
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("m");
    let mut runtime = Session::new(model, Tools::new());
    runtime.run_turn("one").await.expect("the first turn");
    assert_eq!(runtime.history().len(), 2);

    let result = runtime.run_turn("hi").await;

    assert!(
        matches!(result, Err(SessionError::Model { .. })),
        "{result:?}"
    );
    assert_eq!(runtime.history().len(), 2);
}

/// T-6: a turn rolled back after its advisor call does not use up the next
/// turn's call; the advisor sees turn 1, then turn 2.
#[tokio::test]
async fn advisor_limit_survives_a_rolled_back_turn() {
    let stub = chat_stub(vec![
        calling("a1", "advisor"),
        calling("a2", "advisor"),
        answering("ok"),
    ])
    .await;
    let advice = ScriptedProvider::new([text("first advice"), text("second advice")]);
    let turns = Arc::new(Mutex::new(Vec::new()));
    let token = CancellationToken::new();
    let advisor = CountedAdvisor {
        advisor: Advisor::new(scripted(advice)).max_uses(1),
        turns: turns.clone(),
        token: token.clone(),
    };
    let mut runtime = Session::new(stub_model(&stub), Tools::new().with(advisor));

    let first = timeout(WITHIN, runtime.run_turn_cancellable("first", &token))
        .await
        .expect("the turn ends on the cancel");
    interrupted(first);
    let second = timeout(WITHIN, runtime.run_turn("again"))
        .await
        .expect("the turn ends")
        .expect("the second turn succeeds");

    let answer = &second.tool_calls[0].output;
    assert_eq!(answer.content, "second advice");
    assert!(!answer.is_error);
    assert_eq!(second.response, "ok");
    assert_eq!(*turns.lock().unwrap(), [1, 2]);
}

/// T-7: a reply cut at `max_tokens` after a tool step is `Truncated` with
/// the text of both iterations; the turn leaves the history as it was.
#[tokio::test]
async fn truncated_turn_leaves_the_history_empty() {
    let provider = ScriptedProvider::new([
        text_and_calls("plan", vec![tool_use("n1", "note")]),
        vec![
            StreamEvent::ContentDelta("more".into()),
            StreamEvent::Stop(StopReason::MaxTokens),
            StreamEvent::MessageStop,
        ],
    ]);
    let mut runtime = Session::new(
        scripted(provider),
        Tools::new().with(Fixed::ok("note", "done")),
    );

    match runtime.run_turn("go").await {
        Err(error @ SessionError::Truncated { .. }) => {
            assert_eq!(error.turn().response, "planmore");
        }
        other => panic!("expected Truncated, got {other:?}"),
    }

    assert!(
        runtime.history().is_empty(),
        "a truncated turn leaves no trace"
    );
}

/// E-1 (runtime half): `Interrupted` displays as `interrupted`.
#[test]
fn interrupted_displays_as_interrupted() {
    let error = SessionError::Interrupted {
        turn: TurnSummary::default(),
    };

    assert_eq!(error.to_string(), "interrupted");
}

/// A block whose tool `probe` records the turn number and the message count
/// of every call's context.
struct Probe {
    seen: Arc<Mutex<Vec<(u64, usize)>>>,
}

#[crate::agent::tools]
impl Probe {
    /// Records the calling turn and the length of its conversation.
    #[tool]
    async fn probe(&self, context: &CallContext<'_>) -> Result<String, std::convert::Infallible> {
        let seen = (context.turn, context.transcript.messages.len());
        self.seen.lock().unwrap().push(seen);
        Ok("probed".into())
    }
}

/// C1: a `#[tool]` reads the turn number and the conversation from its
/// `CallContext`: turn 1 sees the prompt alone, turn 2 the four messages of
/// turn 1 and its own prompt.
#[tokio::test]
async fn call_context_carries_the_turn_and_the_conversation() {
    let provider = ScriptedProvider::new([
        calls(vec![tool_use("p1", "probe")]),
        text("done"),
        calls(vec![tool_use("p2", "probe")]),
        text("done"),
    ]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tools = Tools::new().with_all(Probe { seen: seen.clone() });
    let mut runtime = Session::new(scripted(provider), tools);

    runtime.run_turn("a").await.expect("the first turn");
    runtime.run_turn("b").await.expect("the second turn");

    assert_eq!(*seen.lock().unwrap(), [(1, 1), (2, 5)]);
}

/// Sends on `done` once `token` fires, read through a `CallContext`.
async fn watch(token: CancellationToken, done: tokio::sync::oneshot::Sender<()>) {
    let context = CallContext {
        cancel: token,
        ..CallContext::default()
    };
    context.cancelled().await;
    done.send(()).expect("the test listens");
}

/// The tool `watch`: spawns [`watch`] on the turn's token, signals
/// `started`, then never returns.
struct Watch {
    started: Arc<Notify>,
    done: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[async_trait]
impl Tool for Watch {
    fn definition(&self) -> ToolDefinition {
        definition("watch")
    }

    async fn call(&self, _input: serde_json::Value, context: &CallContext<'_>) -> ToolOutput {
        let done = self.done.lock().unwrap().take().expect("called once");
        tokio::spawn(watch(context.cancel.clone(), done));
        self.started.notify_one();
        std::future::pending().await
    }
}

/// C2(b): a task a tool spawned on `context.cancel` hears the cancel within
/// 50 ms, and the turn returns `Interrupted`.
#[tokio::test]
async fn spawned_work_hears_the_cancel_through_the_context() {
    let provider = ScriptedProvider::new([calls(vec![tool_use("w1", "watch")])]);
    let started = Arc::new(Notify::new());
    let (done, heard) = tokio::sync::oneshot::channel();
    let tool = Watch {
        started: started.clone(),
        done: Mutex::new(Some(done)),
    };
    let mut runtime = Session::new(scripted(provider), Tools::new().with(tool));
    let token = CancellationToken::new();

    let (result, heard) = timeout(WITHIN, async {
        tokio::join!(runtime.run_turn_cancellable("go", &token), async {
            started.notified().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            token.cancel();
            timeout(Duration::from_millis(50), heard).await
        })
    })
    .await
    .expect("the turn ends on the cancel");

    assert_eq!(heard, Ok(Ok(())));
    assert!(
        matches!(result, Err(SessionError::Interrupted { .. })),
        "{result:?}"
    );
}

// --- Prompt caching ---

/// A complete Anthropic reply that calls the tool `echo` once, as call `t-1`.
fn anthropic_calling() -> StubReply {
    let events = [
        json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":
            {"type":"tool_use","id":"t-1","name":"echo","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":
            {"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},
            "usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ];
    anthropic_reply(&events)
}

/// A complete Anthropic reply that answers "done".
fn anthropic_done() -> StubReply {
    let events = [
        json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":
            {"type":"text_delta","text":"done"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},
            "usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ];
    anthropic_reply(&events)
}

/// `events` as one complete event-stream reply.
fn anthropic_reply(events: &[serde_json::Value]) -> StubReply {
    let body = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .concat();
    StubReply::full(200, "text/event-stream", &body)
}

/// An Anthropic stub that calls `echo`, then answers "done".
async fn echo_then_done() -> StubServer {
    StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![anthropic_calling(), anthropic_done()],
    )])
    .await
}

/// The Anthropic model on `stub`.
pub(super) fn anthropic_model(stub: &StubServer) -> Llm {
    Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .build()
        .llm("claude-x")
}

/// The content blocks of `body` that carry a `cache_control`, as
/// `(message index, block index, marker)`.
fn marked_blocks(body: &serde_json::Value) -> Vec<(usize, usize, serde_json::Value)> {
    let mut found = Vec::new();
    for (m, message) in body["messages"].as_array().unwrap().iter().enumerate() {
        for (b, block) in message["content"].as_array().unwrap().iter().enumerate() {
            if let Some(marker) = block.get("cache_control") {
                found.push((m, b, marker.clone()));
            }
        }
    }
    found
}

/// K3: by default each model call marks exactly the last block of the last
/// message, so the marker rolls forward with the history.
#[tokio::test]
async fn agent_loop_caches_the_last_message_block_by_default() {
    let stub = echo_then_done().await;
    let mut runtime = Session::new(
        anthropic_model(&stub),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );

    runtime.run_turn("hi").await.expect("turn");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let body: serde_json::Value = serde_json::from_str(&request.body).expect("json body");
        let messages = body["messages"].as_array().unwrap();
        let last = messages.len() - 1;
        let last_block = messages[last]["content"].as_array().unwrap().len() - 1;
        assert_eq!(
            marked_blocks(&body),
            vec![(last, last_block, json!({"type":"ephemeral"}))]
        );
    }
}

/// K4: `no_cache()` leaves `cache_control` out of every body.
#[tokio::test]
async fn no_cache_sends_no_markers() {
    let stub = echo_then_done().await;
    let mut runtime = Session::builder(
        anthropic_model(&stub),
        Tools::new().with(Fixed::ok("echo", "hello")),
    )
    .no_cache()
    .build();

    runtime.run_turn("hi").await.expect("turn");

    let requests = stub.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert!(!request.body.contains("cache_control"), "{}", request.body);
    }
}

/// The builder's `cache(ttl)` sets the TTL of the agent's markers.
#[tokio::test]
async fn cache_ttl_reaches_the_agent_markers() {
    let stub = echo_then_done().await;
    let mut runtime = Session::builder(
        anthropic_model(&stub),
        Tools::new().with(Fixed::ok("echo", "hello")),
    )
    .cache(CacheTtl::OneHour)
    .build();

    runtime.run_turn("hi").await.expect("turn");

    let body: serde_json::Value =
        serde_json::from_str(&stub.requests()[0].body).expect("json body");
    assert_eq!(
        marked_blocks(&body)[0].2,
        json!({"type":"ephemeral","ttl":"1h"})
    );
}

// --- Turn usage and failed turns ---

/// An Anthropic reply: `content` events, then `stop_reason`, with the
/// usage `input` / `output` / `cache_read` of one model call.
pub(super) fn anthropic_counted(
    content: &[serde_json::Value],
    stop_reason: &str,
    (input, output, cache_read): (u32, u32, u32),
) -> StubReply {
    let start = json!({"type":"message_start","message":{"usage":{
        "input_tokens":input,"cache_read_input_tokens":cache_read}}});
    let end = json!({"type":"message_delta","delta":{"stop_reason":stop_reason},
        "usage":{"output_tokens":output}});
    let events: Vec<_> = std::iter::once(start)
        .chain(content.iter().cloned())
        .chain([end, json!({"type":"message_stop"})])
        .collect();
    anthropic_reply(&events)
}

/// The content events of a call to `echo`, as call `t-1`.
pub(super) fn echo_call_events() -> Vec<serde_json::Value> {
    vec![
        json!({"type":"content_block_start","index":0,"content_block":
            {"type":"tool_use","id":"t-1","name":"echo","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":
            {"type":"input_json_delta","partial_json":"{}"}}),
        json!({"type":"content_block_stop","index":0}),
    ]
}

/// The content events of the text `text`.
pub(super) fn text_events(text: &str) -> Vec<serde_json::Value> {
    vec![
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":
            {"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
    ]
}

/// U1: the turn's usage is the sum over its two model calls, cache counters
/// included.
#[tokio::test]
async fn turn_usage_sums_the_model_calls() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&echo_call_events(), "tool_use", (100, 10, 50)),
            anthropic_counted(&text_events("done"), "end_turn", (120, 5, 60)),
        ],
    )])
    .await;
    let mut runtime = Session::new(
        anthropic_model(&stub),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );

    let turn = runtime.run_turn("hi").await.expect("turn");

    assert_eq!(turn.usage.input_tokens, 220);
    assert_eq!(turn.usage.output_tokens, 15);
    assert_eq!(turn.usage.cache_read_input_tokens, 110);
}

/// U2: a model error after a tool step carries the turn so far: one tool
/// call, one iteration, the first call's usage; the history is unchanged.
#[tokio::test]
async fn model_error_carries_the_turn_so_far() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&echo_call_events(), "tool_use", (7, 3, 0)),
            StubReply::full(500, "application/json", r#"{"error":{"message":"down"}}"#),
        ],
    )])
    .await;
    let model = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x");
    let mut runtime = Session::new(model, Tools::new().with(Fixed::ok("echo", "hello")));

    let error = runtime.run_turn("hi").await.expect_err("the 500");

    assert!(matches!(error, SessionError::Model { .. }), "{error:?}");
    let turn = error.turn();
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(turn.iterations, 1);
    assert_eq!(turn.usage.input_tokens, 7);
    assert_eq!(turn.usage.output_tokens, 3);
    assert_eq!(runtime.history(), []);
}

/// U3: `MaxIterations` carries the one tool call the turn ran.
#[tokio::test]
async fn max_iterations_carries_its_tool_calls() {
    let tools = Tools::new().with(Fixed::ok("echo", "hello"));
    let mut runtime = Session::builder(scripted(always_echo(1)), tools)
        .max_iterations(1)
        .build();

    let error = runtime.run_turn("hi").await.expect_err("the limit");

    assert!(
        matches!(error, SessionError::MaxIterations { .. }),
        "{error:?}"
    );
    assert_eq!(error.turn().tool_calls.len(), 1);
}

/// U4: `Truncated` carries the text "par" and the call's usage.
#[tokio::test]
async fn truncated_carries_its_text_and_usage() {
    let script = vec![
        StreamEvent::ContentDelta("par".into()),
        StreamEvent::Usage(
            crate::provider::Usage::default()
                .input_tokens(4)
                .output_tokens(2),
        ),
        StreamEvent::Stop(StopReason::MaxTokens),
        StreamEvent::MessageStop,
    ];
    let mut runtime = Session::new(scripted(ScriptedProvider::new([script])), Tools::new());

    let error = runtime.run_turn("hi").await.expect_err("the cut");

    assert!(matches!(error, SessionError::Truncated { .. }), "{error:?}");
    let turn = error.turn();
    assert_eq!(turn.response, "par");
    assert_eq!(turn.usage.input_tokens, 4);
    assert_eq!(turn.usage.output_tokens, 2);
}

/// A tool that takes a number.
#[derive(serde::Deserialize, crate::JsonSchema)]
struct Count {
    /// The number.
    n: u32,
}

/// The tool `count`, which wants `{ "n": <number> }`.
struct Counter;

#[crate::agent::tools]
impl Counter {
    /// Counts to `n`.
    #[tool]
    async fn count(&self, input: Count) -> Result<String, std::convert::Infallible> {
        Ok(format!("counted {}", input.n))
    }
}

/// S7: a call with malformed input reaches the model as an error result and
/// the turn goes on to "ok".
#[tokio::test]
async fn malformed_tool_input_is_an_error_result_and_the_run_goes_on() {
    let provider = ScriptedProvider::new([
        calls(vec![tool_use_with("c1", "count", json!({"n": "x"}))]),
        text("ok"),
    ]);
    let mut runtime = Session::new(scripted(provider.clone()), Tools::new().with_all(Counter));

    let turn = runtime.run_turn("hi").await.expect("the turn survives");

    assert_eq!(turn.response, "ok");
    let requests = provider.requests();
    let last = requests[1]
        .messages
        .last()
        .expect("the tool result message");
    let results = tool_results(&last.content);
    assert_eq!(results.len(), 1);
    assert!(results[0].2, "is_error: {results:?}");
}

// --- Cost and tool timeout ---

/// P3: the cost of the turn's usage equals the sum of its two calls' costs.
#[tokio::test]
async fn turn_cost_is_the_sum_of_the_call_costs() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&echo_call_events(), "tool_use", (100, 10, 50)),
            anthropic_counted(&text_events("done"), "end_turn", (120, 5, 60)),
        ],
    )])
    .await;
    let mut runtime = Session::new(
        anthropic_model(&stub),
        Tools::new().with(Fixed::ok("echo", "hello")),
    );
    let prices = crate::Prices::per_million(3.0, 15.0);
    let first = crate::provider::Usage::default()
        .input_tokens(100)
        .output_tokens(10);
    let second = crate::provider::Usage::default()
        .input_tokens(120)
        .output_tokens(5);

    let turn = runtime.run_turn("hi").await.expect("turn");

    let calls = first.cost(&prices) + second.cost(&prices);
    let cache_reads = f64::from(110) * 0.3 / 1e6;
    assert!((turn.usage.cost(&prices) - (calls + cache_reads)).abs() < 1e-9);
}

/// T1: a tool that sleeps 10 s under `tool_timeout(50 ms)` becomes an error
/// result naming the timeout; the turn goes on and answers "ok".
#[tokio::test(start_paused = true)]
async fn hung_tool_times_out_and_the_turn_goes_on() {
    let provider = ScriptedProvider::new([
        calls(vec![tool_use_with("t-1", "slow", json!({"ms": 10_000}))]),
        text("ok"),
    ]);
    let mut runtime = Session::builder(scripted(provider), Tools::new().with(Slow::default()))
        .tool_timeout(Duration::from_millis(50))
        .build();

    let turn = runtime.run_turn("hi").await.expect("turn");

    assert_eq!(turn.response, "ok");
    let results = tool_results(&runtime.messages[2].content);
    assert_eq!(results.len(), 1);
    let (id, content, is_error) = results[0];
    assert_eq!(id, "t-1");
    assert!(content.contains("timed out"), "{content}");
    assert!(is_error);
}
