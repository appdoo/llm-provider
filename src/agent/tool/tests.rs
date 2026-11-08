//! Tests for `Tools` and the file tools' input helpers.

use async_trait::async_trait;
use serde_json::json;

use super::*;

/// A tool called `name` that answers every call with `reply`.
struct Fixed {
    name: &'static str,
    reply: &'static str,
}

#[async_trait]
impl Tool for Fixed {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.into(),
            description: format!("answers {}", self.reply),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: serde_json::Value) -> ToolOutput {
        ToolOutput {
            content: self.reply.into(),
            is_error: false,
        }
    }
}

fn read_and_write() -> Tools {
    Tools::new()
        .with(Fixed {
            name: "read",
            reply: "read output",
        })
        .with(Fixed {
            name: "write",
            reply: "write output",
        })
}

/// B-1: a call is dispatched by name; an unknown name is an error result.
#[tokio::test]
async fn tools_dispatch_by_name_and_reject_unknown() {
    let tools = read_and_write();

    let write = tools.call("write", json!({})).await;
    let unknown = tools.call("nope", json!({})).await;

    assert_eq!(
        write,
        ToolOutput {
            content: "write output".into(),
            is_error: false,
        }
    );
    assert_eq!(
        unknown,
        ToolOutput {
            content: "unknown tool nope".into(),
            is_error: true,
        }
    );
}

/// `definitions` lists every tool in the order it was added.
#[test]
fn tools_definitions_in_order() {
    let names: Vec<String> = read_and_write()
        .definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect();

    assert_eq!(names, ["read", "write"]);
}

#[cfg(feature = "fs-tools")]
mod input_helpers {
    use serde_json::json;

    use super::super::{optional_u64_clamped, require_string, require_string_nonempty};

    #[test]
    fn require_string_nonempty_missing_field() {
        let err = require_string_nonempty(&json!({}), "X").unwrap_err();
        assert_eq!(err, "field 'X' is required and must be a string");
    }

    #[test]
    fn optional_u64_clamped_below_min() {
        assert_eq!(optional_u64_clamped(&json!({ "t": 0 }), "t", 30, 1, 300), 1);
    }

    #[test]
    fn optional_u64_clamped_above_max() {
        let input = json!({ "t": 99_999u64 });
        assert_eq!(optional_u64_clamped(&input, "t", 30, 1, 300), 300);
    }

    #[test]
    fn require_string_nonempty_returns_value() {
        let input = json!({ "path": "/tmp/x" });
        assert_eq!(require_string_nonempty(&input, "path").unwrap(), "/tmp/x");
    }

    #[test]
    fn require_string_nonempty_empty_value() {
        let err = require_string_nonempty(&json!({ "path": "" }), "path").unwrap_err();
        assert_eq!(err, "field 'path' is required and must not be empty");
    }

    #[test]
    fn optional_u64_clamped_uses_default_when_absent() {
        assert_eq!(optional_u64_clamped(&json!({}), "t", 30, 1, 300), 30);
    }

    #[test]
    fn optional_u64_clamped_passes_through_in_range() {
        assert_eq!(
            optional_u64_clamped(&json!({ "t": 42 }), "t", 30, 1, 300),
            42
        );
    }

    #[test]
    fn require_string_wrong_type() {
        let err = require_string(&json!({ "x": 5 }), "x").unwrap_err();
        assert_eq!(err, "field 'x' is required and must be a string");
    }
}
