//! Tests for `Tools`.

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

    async fn call(&self, _input: serde_json::Value, _transcript: &Transcript<'_>) -> ToolOutput {
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

    let write = tools.call("write", json!({}), &Transcript::default()).await;
    let unknown = tools.call("nope", json!({}), &Transcript::default()).await;

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
