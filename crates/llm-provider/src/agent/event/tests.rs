//! Tests for the stored event format (Spec 63, G1, G2, T1).

use super::*;
use crate::provider::{Message, MessageRole};

/// The session id of the golden events.
fn id() -> Uuid {
    Uuid::parse_str("0194f4c0-0000-7000-8000-000000000001").expect("a valid UUID")
}

fn usage(input_tokens: u32, output_tokens: u32, cache_write: u32, cache_read: u32) -> Usage {
    Usage {
        input_tokens,
        output_tokens,
        cache_creation_input_tokens: cache_write,
        cache_creation_1h_input_tokens: 0,
        cache_read_input_tokens: cache_read,
    }
}

/// One fixed event per variant, plus a failed call, in the order of
/// `tests/fixtures/session_events.jsonl`.
fn fixed_events() -> Vec<SessionEvent> {
    vec![
        SessionEvent::ModelCall {
            session_id: id(),
            seq: 0,
            turn: 1,
            iteration: 1,
            model: "claude-x".into(),
            provider: "anthropic".into(),
            started_at_ms: 1_700_000_000_000,
            duration_ms: 1234,
            stop_reason: Some(StopReason::EndTurn),
            messages: vec![Message::user("hi"), Message::assistant("a")],
            usage: usage(100, 10, 5, 50),
            error: None,
            setup: Some(SessionSetup {
                system: Some("Be brief.".into()),
                tools: vec!["echo".into()],
            }),
        },
        SessionEvent::ModelCall {
            session_id: id(),
            seq: 1,
            turn: 2,
            iteration: 1,
            model: "claude-x".into(),
            provider: "anthropic".into(),
            started_at_ms: 1_700_000_001_000,
            duration_ms: 20,
            stop_reason: None,
            messages: vec![Message::user("again")],
            usage: Usage::default(),
            error: Some(EventError {
                kind: ErrorKind::Server,
                message: "down".into(),
            }),
            setup: None,
        },
        SessionEvent::Compacted {
            session_id: id(),
            seq: 2,
            turn: 3,
            before: 3,
            after: 1,
            history: vec![Message::new(
                MessageRole::User,
                vec![crate::provider::Content::Text { text: "S".into() }],
            )],
        },
        SessionEvent::TurnEnded {
            session_id: id(),
            seq: 3,
            turn: 3,
            ok: true,
            usage: usage(120, 5, 0, 0),
        },
    ]
}

const GOLDEN: &str = include_str!("../../../tests/fixtures/session_events.jsonl");

/// G1: each fixed event serializes to its golden line byte for byte, and the
/// time fields are JSON integers.
#[test]
fn events_serialize_to_the_golden_lines() {
    let lines: Vec<&str> = GOLDEN.lines().collect();
    let events = fixed_events();
    assert_eq!(lines.len(), events.len());

    for (event, line) in events.iter().zip(&lines) {
        assert_eq!(&serde_json::to_string(event).expect("serialize"), line);
    }

    let value = serde_json::to_value(&events[0]).expect("serialize");
    assert_eq!(
        value["started_at_ms"],
        serde_json::json!(1_700_000_000_000_u64)
    );
    assert!(value["started_at_ms"].is_u64());
    assert_eq!(value["duration_ms"], serde_json::json!(1234));
    assert!(value["duration_ms"].is_u64());
}

/// G2: each golden line reads back as the fixed event.
#[test]
fn golden_lines_deserialize_to_the_fixed_events() {
    let events = fixed_events();

    for (line, event) in GOLDEN.lines().zip(&events) {
        let read: SessionEvent = serde_json::from_str(line).expect("a stored line reads");
        assert_eq!(&read, event);
    }
}

/// T1: `turn()` is the turn number of every variant; a `TurnEnded` of turn 3
/// says 3.
#[test]
fn turn_is_the_turn_number() {
    let turns: Vec<u64> = fixed_events().iter().map(SessionEvent::turn).collect();

    assert_eq!(turns, [1, 2, 3, 3]);
}
