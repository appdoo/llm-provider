//! Tests for the message types: wire shape, round trip, constructors.

use super::*;

/// A text message serializes to the exact wire shape and round-trips.
#[test]
fn text_message_serializes_to_wire_shape() {
    let msg = Message {
        role: MessageRole::User,
        content: vec![Content::Text {
            text: "Hello".into(),
        }],
    };

    let json = serde_json::to_string(&msg).expect("serialize");
    assert_eq!(
        json,
        r#"{"role":"user","content":[{"type":"text","text":"Hello"}]}"#
    );

    let back: Message = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, msg);
}

/// Text, `tool_use` and `tool_result` survive a JSON round-trip.
#[test]
fn mixed_content_message_round_trips() {
    let msg = Message {
        role: MessageRole::Assistant,
        content: vec![
            Content::Text { text: "a".into() },
            Content::ToolUse {
                id: "toolu_1".into(),
                name: "Read".into(),
                input: serde_json::json!({"path": "a.txt"}),
            },
            Content::ToolResult {
                tool_use_id: "toolu_1".into(),
                content: "ok".into(),
                is_error: false,
            },
        ],
    };

    let json = serde_json::to_string(&msg).expect("serialize");
    let back: Message = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, msg);
}

/// The message constructors build one text block.
#[test]
fn message_constructors() {
    assert_eq!(
        Message::user("x"),
        Message {
            role: MessageRole::User,
            content: vec![Content::Text { text: "x".into() }],
        }
    );
    assert_eq!(
        Message::assistant("y"),
        Message {
            role: MessageRole::Assistant,
            content: vec![Content::Text { text: "y".into() }],
        }
    );
}
