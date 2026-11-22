//! Tests for restoring a session from its events (Spec 63, F1–F5).

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{self, Receiver};

use crate::agent::runtime::tests::{anthropic_counted, anthropic_model, text_events};
use crate::agent::test_support::scripted;
use crate::agent::{Session, SessionEvent, Tools, Uuid};
use crate::provider::test_stub::StubServer;
use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Content, Message, MessageRole, Usage};

// --- Helpers ---

/// The session id of the hand-written logs.
fn id() -> Uuid {
    Uuid::parse_str("0194f4c0-0000-7000-8000-000000000002").expect("a valid UUID")
}

/// A model-call event of `turn` that read `input` tokens and added `messages`.
fn model_call(seq: u64, turn: u64, input: u32, messages: Vec<Message>) -> SessionEvent {
    SessionEvent::ModelCall {
        session_id: id(),
        seq,
        turn,
        iteration: 1,
        model: "claude-x".into(),
        provider: "anthropic".into(),
        started_at_ms: 1_700_000_000_000,
        duration_ms: 10,
        stop_reason: None,
        messages,
        usage: Usage::default().input_tokens(input),
        error: None,
        setup: None,
    }
}

fn turn_ended(seq: u64, turn: u64, ok: bool) -> SessionEvent {
    SessionEvent::TurnEnded {
        session_id: id(),
        seq,
        turn,
        ok,
        usage: Usage::default(),
    }
}

fn compacted(seq: u64, turn: u64, history: Vec<Message>) -> SessionEvent {
    SessionEvent::Compacted {
        session_id: id(),
        seq,
        turn,
        before: 2,
        after: history.len(),
        history,
    }
}

/// A stub that answers every call with "ok".
async fn ok_stub() -> StubServer {
    StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![anthropic_counted(
            &text_events("ok"),
            "end_turn",
            (50, 5, 0),
        )],
    )])
    .await
}

/// The `(role, first text)` of every message of the stub's first request.
fn sent(stub: &StubServer) -> Vec<(String, String)> {
    let body: serde_json::Value = serde_json::from_str(&stub.requests()[0].body).expect("json");
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .map(|m| {
            let text = m["content"][0]["text"].as_str().unwrap_or("<block>");
            (m["role"].as_str().expect("role").into(), text.into())
        })
        .collect()
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(role, text)| ((*role).into(), (*text).into()))
        .collect()
}

fn drain(rx: &mut Receiver<SessionEvent>) -> Vec<SessionEvent> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

/// A session on `stub` restored from `events`, with its event receiver.
fn restored(
    stub: &StubServer,
    events: Vec<SessionEvent>,
) -> (Session<Tools>, Receiver<SessionEvent>) {
    let (tx, rx) = mpsc::channel(64);
    let session = Session::builder(anthropic_model(stub), Tools::new())
        .restore(events)
        .events(tx)
        .build();
    (session, rx)
}

// --- Restore ---

/// F1: the events of two ok turns restore the four messages; the next turn
/// sends them and "x", and its event is seq 4, turn 3 with the old id.
#[tokio::test]
async fn two_ok_turns_restore_to_their_history() {
    let stub = StubServer::start_replies(vec![(
        ("POST", "/v1/messages"),
        vec![
            anthropic_counted(&text_events("a"), "end_turn", (10, 1, 0)),
            anthropic_counted(&text_events("b"), "end_turn", (20, 1, 0)),
        ],
    )])
    .await;
    let (tx, mut rx) = mpsc::channel(64);
    let mut original = Session::builder(anthropic_model(&stub), Tools::new())
        .events(tx)
        .build();
    original.run_turn("hi").await.expect("turn 1");
    original.run_turn("again").await.expect("turn 2");
    let events = drain(&mut rx);
    assert_eq!(
        events.iter().map(SessionEvent::seq).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );

    let next = ok_stub().await;
    let (mut session, mut rx) = restored(&next, events);
    assert_eq!(session.history(), original.history());
    assert_eq!(session.session_id(), original.session_id());
    session.run_turn("x").await.expect("turn 3");

    assert_eq!(
        sent(&next),
        pairs(&[
            ("user", "hi"),
            ("assistant", "a"),
            ("user", "again"),
            ("assistant", "b"),
            ("user", "x"),
        ])
    );
    let first = &drain(&mut rx)[0];
    assert_eq!((first.seq(), first.turn()), (4, 3));
    assert_eq!(first.session_id(), original.session_id());
}

/// F2: a turn 2 that crashed after a `tool_use` event, with no `TurnEnded`,
/// is left out: the request holds turn 1's two messages and "q2", and the
/// new event is turn 3, seq 3.
#[tokio::test]
async fn a_turn_without_an_end_is_left_out() {
    let tool_use = Message::new(
        MessageRole::Assistant,
        vec![Content::ToolUse {
            id: "t-1".into(),
            name: "echo".into(),
            input: serde_json::json!({}),
        }],
    );
    let events = vec![
        model_call(0, 1, 10, vec![Message::user("hi"), Message::assistant("a")]),
        turn_ended(1, 1, true),
        model_call(2, 2, 10, vec![Message::user("q"), tool_use]),
    ];
    let stub = ok_stub().await;
    let (mut session, mut rx) = restored(&stub, events);

    session.run_turn("q2").await.expect("turn");

    assert_eq!(
        sent(&stub),
        pairs(&[("user", "hi"), ("assistant", "a"), ("user", "q2")])
    );
    let next = &drain(&mut rx)[0];
    assert_eq!((next.turn(), next.seq()), (3, 3));
}

/// F3: a `Compacted` after turn 1 replaces the earlier history; the history
/// is "S", then turn 2's messages.
#[test]
fn the_last_compaction_starts_the_history() {
    let events = vec![
        model_call(0, 1, 10, vec![Message::user("hi"), Message::assistant("a")]),
        turn_ended(1, 1, true),
        compacted(2, 2, vec![Message::user("S")]),
        model_call(
            3,
            2,
            10,
            vec![Message::user("q2"), Message::assistant("r2")],
        ),
        turn_ended(4, 2, true),
    ];
    let (tx, _rx) = mpsc::channel(1);
    let session = Session::builder(scripted(ScriptedProvider::new([])), Tools::new())
        .restore(events)
        .events(tx)
        .build();

    assert_eq!(
        session.history(),
        [
            Message::user("S"),
            Message::user("q2"),
            Message::assistant("r2")
        ]
    );
}

/// F4: a log whose last model call read 1200 input tokens compacts at the
/// start of the first turn after the restore, before its first call.
#[tokio::test]
async fn the_last_input_decides_the_first_compaction() {
    let events = vec![
        model_call(
            0,
            1,
            1200,
            vec![Message::user("hi"), Message::assistant("a")],
        ),
        turn_ended(1, 1, true),
    ];
    let stub = ok_stub().await;
    let seen = Arc::new(Mutex::new(Vec::<Vec<Message>>::new()));
    let record = Arc::clone(&seen);
    let mut session = Session::builder(anthropic_model(&stub), Tools::new())
        .restore(events)
        .compact_when(1000, move |history| {
            let record = Arc::clone(&record);
            async move {
                record.lock().expect("lock").push(history);
                Ok(vec![Message::user("S")])
            }
        })
        .build();

    session.run_turn("x").await.expect("turn");

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], [Message::user("hi"), Message::assistant("a")]);
    assert_eq!(sent(&stub), pairs(&[("user", "S"), ("user", "x")]));
}

/// F5: no events give a fresh version 7 id, an empty history and seq 0.
#[tokio::test]
async fn no_events_give_a_fresh_session() {
    let stub = ok_stub().await;
    let (mut session, mut rx) = restored(&stub, Vec::new());

    assert_eq!(session.session_id().get_version_num(), 7);
    assert_eq!(session.history(), []);
    session.run_turn("x").await.expect("turn");

    let first = &drain(&mut rx)[0];
    assert_eq!((first.seq(), first.turn()), (0, 1));
}
