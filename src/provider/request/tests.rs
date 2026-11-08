//! Tests for `CompletionRequest`: the shared message buffer and the JSON
//! shape.

use super::*;
use crate::provider::message::{Content, MessageRole};

/// A request with `messages` and the given model.
fn request(model: &str, system: Option<&str>, messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: ModelSelection::Named(model.to_string()),
        system: system.map(str::to_string),
        messages: Arc::from(messages),
        tools: vec![],
        max_tokens: 1024,
        cache_breakpoints: Vec::new(),
        response_schema: None,
    }
}

/// A clone shares the message buffer.
#[test]
fn completion_request_arc_messages_shares_buffer_on_clone() {
    let req = request("claude-sonnet-5-5", None, vec![Message::user("hello")]);

    let cloned = req.clone();

    assert!(Arc::ptr_eq(&req.messages, &cloned.messages));
    assert_eq!(req.messages.len(), 1);
}

/// The JSON matches the golden snapshot and round-trips byte for byte; an
/// empty message slice is `[]`.
#[test]
fn completion_request_json_roundtrip_matches_golden() {
    let tool_use = Message {
        role: MessageRole::User,
        content: vec![Content::ToolUse {
            id: "toolu_1".to_string(),
            name: "bash".to_string(),
            input: serde_json::json!({"cmd": "ls"}),
        }],
    };
    let req = request(
        "claude-sonnet-5-5",
        Some("You are helpful."),
        vec![tool_use],
    );
    let golden = r#"{"model":{"named":"claude-sonnet-5-5"},"system":"You are helpful.","messages":[{"role":"user","content":[{"type":"tool_use","id":"toolu_1","name":"bash","input":{"cmd":"ls"}}]}],"tools":[],"max_tokens":1024}"#;

    let encoded = serde_json::to_string(&req).expect("serialize");
    assert_eq!(encoded, golden);

    let decoded: CompletionRequest = serde_json::from_str(&encoded).expect("deserialize");
    assert_eq!(
        serde_json::to_string(&decoded).expect("re-serialize"),
        golden
    );

    let empty = serde_json::to_string(&request("m", None, vec![])).expect("serialize");
    assert!(empty.contains(r#""messages":[]"#), "got {empty}");
}

/// `ResponseSchema::of` names the schema after the type and keeps the
/// field docs, without `$schema` or `title`.
#[cfg(feature = "schema")]
#[test]
fn response_schema_from_a_type() {
    let schema = ResponseSchema::of::<crate::provider::test_support::Summary>();

    assert_eq!(schema.name(), "Summary");
    let json = schema.schema();
    assert_eq!(
        json["required"],
        serde_json::json!(["sentences", "key_points"])
    );
    assert_eq!(
        json["properties"]["sentences"]["description"],
        "Two to four sentences."
    );
    assert!(
        json.get("$schema").is_none() && json.get("title").is_none(),
        "{json}"
    );
}
