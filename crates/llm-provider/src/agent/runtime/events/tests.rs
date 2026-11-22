//! Tests for the session events and compaction (Spec 62, E1–E6, R1, C1, C2), against the
//! Anthropic stub.

use std::sync::Arc;

use async_trait::async_trait;

use tokio::sync::mpsc::{self, Receiver};
use tokio::sync::Notify;

use super::*;
use crate::agent::runtime::tests::{
    anthropic_counted, anthropic_model, echo_call_events, text_events, Fixed,
};
use crate::agent::{CallContext, SessionEvent, Tool, ToolOutput, Tools, Uuid};
use crate::provider::test_stub::{StubReply, StubServer};
use crate::provider::{
    Anthropic, Content, ErrorKind, MessageRole, RetryPolicy, StopReason, ToolDefinition, Usage,
};

// --- Helpers ---

/// The fields of a model-call event the tests read; the one place that
/// knows the variant.
struct Call<'a> {
    session_id: Uuid,
    seq: u64,
    turn: u64,
    iteration: usize,
    started_at_ms: u64,
    stop_reason: Option<StopReason>,
    messages: &'a [Message],
    usage: Usage,
    error: Option<&'a crate::agent::EventError>,
}

fn call(event: &SessionEvent) -> Call<'_> {
    let SessionEvent::ModelCall {
        session_id,
        seq,
        turn,
        iteration,
        started_at_ms,
        stop_reason,
        messages,
        usage,
        error,
        ..
    } = event
    else {
        panic!("a model call, not {event:?}");
    };
    Call {
        session_id: *session_id,
        seq: *seq,
        turn: *turn,
        iteration: *iteration,
        started_at_ms: *started_at_ms,
        stop_reason: *stop_reason,
        messages,
        usage: *usage,
        error: error.as_ref(),
    }
}

/// Every event the receiver holds right now.
fn drain(rx: &mut Receiver<SessionEvent>) -> Vec<SessionEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// The model-call events the receiver holds right now; turn ends and
/// compactions are left out.
fn drain_calls(rx: &mut Receiver<SessionEvent>) -> Vec<SessionEvent> {
    drain(rx)
        .into_iter()
        .filter(|event| matches!(event, SessionEvent::ModelCall { .. }))
        .collect()
}

/// A stub that answers the turn "hi" with a call to `echo` (in 100, out 10,
/// cache read 50) and then "done" (in 120, out 5), and any further call
/// with "ok" (in 130, out 4).
async fn stub() -> StubServer {
    StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&echo_call_events(), "tool_use", (100, 10, 50)),
            anthropic_counted(&text_events("done"), "end_turn", (120, 5, 0)),
            anthropic_counted(&text_events("ok"), "end_turn", (130, 4, 0)),
        ],
    )])
    .await
}

fn echo() -> Tools {
    Tools::new().with(Fixed::ok("echo", "hello"))
}

/// The tool `echo`, which answers "hello" only after `gate` was notified.
struct Gated(Arc<Notify>);

#[async_trait]
impl Tool for Gated {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".into(),
            description: "the echo tool".into(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
        self.0.notified().await;
        ToolOutput {
            content: "hello".into(),
            is_error: false,
        }
    }
}

fn tool_use_message() -> Message {
    Message::new(
        MessageRole::Assistant,
        vec![Content::ToolUse {
            id: "t-1".into(),
            name: "echo".into(),
            input: serde_json::json!({}),
        }],
    )
}

fn tool_result_message() -> Message {
    Message::new(
        MessageRole::User,
        vec![Content::ToolResult {
            tool_use_id: "t-1".into(),
            content: "hello".into(),
            is_error: false,
        }],
    )
}

/// Runs the turn "hi" of E1 and returns the runtime, its stub and its events.
async fn first_turn() -> (Session<Tools>, StubServer, Vec<SessionEvent>) {
    let stub = stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(anthropic_model(&stub), echo())
        .events(tx)
        .build();

    runtime.run_turn("hi").await.expect("turn");

    let events = drain(&mut rx);
    (runtime, stub, events)
}

// --- Events ---

/// E1: a tool call and then "done" make two events with seq 0 and 1, the
/// runtime's version 7 id, turn 1, iterations 1 and 2, each call's own
/// usage and its added messages; their usage sums to the turn's.
#[tokio::test]
async fn one_event_per_model_call() {
    let stub = stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(anthropic_model(&stub), echo())
        .events(tx)
        .build();

    let turn = runtime.run_turn("hi").await.expect("turn");

    let events = drain_calls(&mut rx);
    assert_eq!(events.len(), 2);
    let (first, second) = (call(&events[0]), call(&events[1]));
    assert_eq!((first.seq, second.seq), (0, 1));
    assert_eq!(first.session_id, runtime.session_id());
    assert_eq!(second.session_id, runtime.session_id());
    assert_eq!(runtime.session_id().get_version_num(), 7);
    assert_eq!((first.turn, second.turn), (1, 1));
    assert_eq!((first.iteration, second.iteration), (1, 2));
    assert_eq!(first.stop_reason, Some(StopReason::ToolUse));
    assert_eq!(second.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(
        (
            first.usage.input_tokens,
            first.usage.output_tokens,
            first.usage.cache_read_input_tokens
        ),
        (100, 10, 50)
    );
    assert_eq!(
        (second.usage.input_tokens, second.usage.output_tokens),
        (120, 5)
    );
    assert_eq!(first.messages, [Message::user("hi"), tool_use_message()]);
    assert_eq!(
        second.messages,
        [tool_result_message(), Message::assistant("done")]
    );
    let mut sum = Usage::default();
    sum += first.usage;
    sum += second.usage;
    assert_eq!(sum, turn.usage);
}

/// E2: a second turn continues the seq (0, 1, then 3 after turn 1's turn end) and numbers its event turn
/// 2; every event has a start time.
#[tokio::test]
async fn seq_runs_across_turns() {
    let stub = stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(anthropic_model(&stub), echo())
        .events(tx)
        .build();

    runtime.run_turn("hi").await.expect("turn 1");
    runtime.run_turn("again").await.expect("turn 2");

    let events = drain_calls(&mut rx);
    let calls: Vec<_> = events.iter().map(call).collect();
    // seq 2 is turn 1's `TurnEnded`.
    assert_eq!(calls.iter().map(|c| c.seq).collect::<Vec<_>>(), [0, 1, 3]);
    assert_eq!(calls[2].turn, 2);
    assert!(calls.iter().all(|c| c.started_at_ms > 0));
}

/// E3: a receiver dropped after the first event fails the turn as `Store`
/// with the first call's tool call in `turn()` and the history unchanged.
#[tokio::test]
async fn a_closed_receiver_fails_the_turn() {
    let stub = stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let closed = Arc::new(Notify::new());
    let signal = Arc::clone(&closed);
    tokio::spawn(async move {
        let _first = rx.recv().await;
        drop(rx);
        signal.notify_one();
    });
    // The tool runs between the two events and waits for the drop.
    let mut runtime = Session::builder(anthropic_model(&stub), Tools::new().with(Gated(closed)))
        .events(tx)
        .build();

    let error = runtime
        .run_turn("hi")
        .await
        .expect_err("the closed channel");

    assert!(matches!(error, SessionError::Store { .. }), "{error:?}");
    assert_eq!(error.turn().tool_calls.len(), 1);
    assert_eq!(runtime.history(), []);
}

/// E4: a 500 without retries sends one event with the error and no reply
/// message, then the turn fails as `Model`.
#[tokio::test]
async fn a_failed_call_is_recorded() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![StubReply::full(
            500,
            "application/json",
            r#"{"error":{"message":"down"}}"#,
        )],
    )])
    .await;
    let model = Anthropic::builder("sk-test")
        .base_url(stub.url())
        .expect("a valid URL")
        .retry(RetryPolicy::none())
        .build()
        .llm("claude-x");
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(model, echo()).events(tx).build();

    let error = runtime.run_turn("hi").await.expect_err("the 500");

    assert!(matches!(error, SessionError::Model { .. }), "{error:?}");
    let events = drain_calls(&mut rx);
    assert_eq!(events.len(), 1);
    let failed = call(&events[0]);
    let recorded = failed.error.expect("the error is recorded");
    assert_eq!(recorded.kind, ErrorKind::Server);
    assert!(recorded.message.contains("down"), "{}", recorded.message);
    assert_eq!(failed.stop_reason, None);
    assert_eq!(failed.messages, [Message::user("hi")]);
}

/// E5: the events of E1 survive a JSON round trip unchanged.
#[tokio::test]
async fn events_round_trip_through_json() {
    let (_runtime, _stub, events) = first_turn().await;
    assert_eq!(events.len(), 3);

    let json = serde_json::to_string(&events).expect("serialize");
    let back: Vec<SessionEvent> = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back, events);
}

/// S1: with the receiver dropped before the second event, the `Store` error's
/// turn holds the paid-for second call too: its text "done", iterations 2 and
/// input tokens 100 + 120, next to the first call's tool call.
#[tokio::test]
async fn a_store_error_keeps_the_call_that_was_paid_for() {
    let stub = stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let closed = Arc::new(Notify::new());
    let signal = Arc::clone(&closed);
    tokio::spawn(async move {
        let _first = rx.recv().await;
        drop(rx);
        signal.notify_one();
    });
    let mut runtime = Session::builder(anthropic_model(&stub), Tools::new().with(Gated(closed)))
        .events(tx)
        .build();

    let error = runtime
        .run_turn("hi")
        .await
        .expect_err("the closed channel");

    assert!(matches!(error, SessionError::Store { .. }), "{error:?}");
    let turn = error.turn();
    assert_eq!(turn.iterations, 2);
    assert_eq!(turn.usage.input_tokens, 220);
    assert_eq!(turn.response, "done");
    assert_eq!(turn.tool_calls.len(), 1);
}

// --- Restore ---

/// R1: a runtime restored from E1's events (history, id, seq 3, one turn) sends
/// the four earlier messages and then "again", and its event carries seq 3,
/// turn 2 and the old id; two fresh runtimes have different ids.
#[tokio::test]
async fn restore_continues_the_stored_session() {
    let (first, _stub, events) = first_turn().await;
    let id = first.session_id();
    let history: Vec<Message> = events
        .iter()
        .filter(|event| matches!(event, SessionEvent::ModelCall { .. }))
        .flat_map(|event| call(event).messages.to_vec())
        .collect();
    assert_eq!(history.len(), 4);

    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![anthropic_counted(
            &text_events("ok"),
            "end_turn",
            (130, 4, 0),
        )],
    )])
    .await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut restored = Session::builder(anthropic_model(&stub), echo())
        .restore(events)
        .events(tx)
        .build();

    restored.run_turn("again").await.expect("turn");

    assert_eq!(restored.session_id(), id);
    let body: serde_json::Value = serde_json::from_str(&stub.requests()[0].body).expect("json");
    let sent = body["messages"].as_array().expect("messages");
    let roles: Vec<_> = sent.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);
    assert_eq!(sent[0]["content"][0]["text"], "hi");
    assert_eq!(sent[4]["content"][0]["text"], "again");
    let events = drain_calls(&mut rx);
    assert_eq!(events.len(), 1);
    let next = call(&events[0]);
    assert_eq!((next.seq, next.turn, next.session_id), (3, 2, id));

    let other = Session::new(anthropic_model(&stub), echo());
    assert_ne!(other.session_id(), first.session_id());
}

// --- Turn end ---

/// E6: turn 1 answered "ok" ends with `TurnEnded` ok and its usage, turn 2
/// failed by a 500 ends with `TurnEnded` not ok; each is the last event of
/// its turn.
#[tokio::test]
async fn every_turn_ends_with_a_turn_end() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&text_events("ok"), "end_turn", (130, 4, 0)),
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
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(model, echo()).events(tx).build();

    runtime.run_turn("hi").await.expect("turn 1");
    runtime.run_turn("again").await.expect_err("the 500");

    let events = drain(&mut rx);
    assert_eq!(events.len(), 4, "{events:?}");
    assert_eq!(call(&events[0]).turn, 1);
    let SessionEvent::TurnEnded {
        session_id,
        seq,
        turn,
        ok,
        usage,
    } = &events[1]
    else {
        panic!("a TurnEnded event, not {:?}", events[1]);
    };
    assert_eq!(
        (*session_id, *seq, *turn, *ok),
        (runtime.session_id(), 1, 1, true)
    );
    assert_eq!((usage.input_tokens, usage.output_tokens), (130, 4));
    assert_eq!(call(&events[2]).turn, 2);
    let SessionEvent::TurnEnded { seq, turn, ok, .. } = &events[3] else {
        panic!("a TurnEnded event, not {:?}", events[3]);
    };
    assert_eq!((*seq, *turn, *ok), (3, 2, false));
}

// --- Compaction ---

/// A stub that answers the turn "hi" with "a" having read 1200 input
/// tokens, and any further call with "b".
async fn big_stub() -> StubServer {
    StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&text_events("a"), "end_turn", (1200, 10, 0)),
            anthropic_counted(&text_events("b"), "end_turn", (50, 5, 0)),
        ],
    )])
    .await
}

/// C1: after turn 1 read 1200 input tokens, turn 2 starts by handing the
/// history to the callback; the request holds "S" and "again", a `Compacted`
/// event (before 2, after 1) comes before turn 2's call, and the history
/// ends as "S", "again", "b".
#[tokio::test]
async fn a_long_history_is_compacted_at_the_start_of_the_next_turn() {
    let stub = big_stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let seen = Arc::new(std::sync::Mutex::new(Vec::<Vec<Message>>::new()));
    let record = Arc::clone(&seen);
    let mut runtime = Session::builder(anthropic_model(&stub), echo())
        .compact_when(1000, move |history| {
            let record = Arc::clone(&record);
            async move {
                record.lock().expect("lock").push(history);
                Ok(vec![Message::user("S")])
            }
        })
        .events(tx)
        .build();

    runtime.run_turn("hi").await.expect("turn 1");
    assert!(seen.lock().expect("lock").is_empty());
    runtime.run_turn("again").await.expect("turn 2");

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], [Message::user("hi"), Message::assistant("a")]);
    let body: serde_json::Value = serde_json::from_str(&stub.requests()[1].body).expect("json");
    let sent = body["messages"].as_array().expect("messages");
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0]["content"][0]["text"], "S");
    assert_eq!(sent[1]["content"][0]["text"], "again");
    let events = drain(&mut rx);
    assert_eq!(events.len(), 5, "{events:?}");
    let SessionEvent::Compacted {
        seq,
        turn,
        before,
        after,
        history,
        ..
    } = &events[2]
    else {
        panic!("a Compacted event, not {:?}", events[2]);
    };
    assert_eq!((*seq, *turn, *before, *after), (2, 2, 2, 1));
    assert_eq!(history, &[Message::user("S")]);
    assert_eq!(call(&events[3]).seq, 3);
    assert_eq!(
        runtime.history(),
        [
            Message::user("S"),
            Message::user("again"),
            Message::assistant("b")
        ]
    );
}

/// C2: a failing callback ends turn 2 as `Compact` with the history
/// unchanged, no second model call and no `Compacted` event.
#[tokio::test]
async fn a_failing_callback_fails_the_turn() {
    let stub = big_stub().await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut runtime = Session::builder(anthropic_model(&stub), echo())
        .compact_when(1000, |_history| async { Err("boom".into()) })
        .events(tx)
        .build();

    runtime.run_turn("hi").await.expect("turn 1");
    let error = runtime.run_turn("again").await.expect_err("the callback");

    assert!(matches!(error, SessionError::Compact { .. }), "{error:?}");
    assert!(error.to_string().contains("boom"), "{error}");
    assert_eq!(
        runtime.history(),
        [Message::user("hi"), Message::assistant("a")]
    );
    assert_eq!(stub.requests().len(), 1);
    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, SessionEvent::Compacted { .. })),
        "{events:?}"
    );
}

// --- Cleanups ---

/// D1: the default `Session` names its toolbox type (`Tools`).
#[test]
fn session_defaults_to_tools() {
    fn takes(_session: &mut Session) {}
    let _ = takes;
}
