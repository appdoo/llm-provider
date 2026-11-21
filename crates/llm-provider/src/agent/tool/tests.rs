//! Tests for `Tools`.

use std::time::Duration;

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

    async fn call(&self, _input: serde_json::Value, _context: &CallContext<'_>) -> ToolOutput {
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

    let write = tools
        .call("write", json!({}), &CallContext::default())
        .await;
    let unknown = tools.call("nope", json!({}), &CallContext::default()).await;

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

/// The tool `t`, which answers with the rendered transcript it is handed.
struct Renders;

#[async_trait]
impl Tool for Renders {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "t".into(),
            description: "renders the transcript".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _input: serde_json::Value, context: &CallContext<'_>) -> ToolOutput {
        ToolOutput {
            content: context.transcript.render(),
            is_error: false,
        }
    }
}

/// `render` labels the system prompt, the tools and every block in order.
#[test]
fn render_labels_every_part_in_order() {
    let tools = [Renders.definition()];
    let messages = [
        Message::user("go"),
        Message {
            role: MessageRole::Assistant,
            content: vec![Content::ToolUse {
                id: "c1".into(),
                name: "t".into(),
                input: json!({ "x": 1 }),
            }],
        },
        Message {
            role: MessageRole::User,
            content: vec![Content::ToolResult {
                tool_use_id: "c1".into(),
                content: "no".into(),
                is_error: true,
            }],
        },
        Message::assistant("done"),
    ];
    let transcript = Transcript {
        system: Some("S"),
        tools: &tools,
        messages: &messages,
    };

    assert_eq!(
        transcript.render(),
        "System prompt:\nS\n\nTools: t\n\nUser:\ngo\n\nTool call t (c1):\n{\"x\":1}\n\n\
         Tool error (c1):\nno\n\nAssistant:\ndone"
    );
}

/// X-1: a tool sees the calling runtime's system prompt, its tools and the
/// user prompt, in that order.
#[tokio::test]
async fn tool_reads_the_calling_conversation() {
    use crate::agent::test_support::{calls, scripted, text, tool_use};
    use crate::agent::ConversationRuntime;
    use crate::provider::test_support::ScriptedProvider;

    let model = ScriptedProvider::new([calls(vec![tool_use("c1", "t")]), text("ok")]);
    let mut runtime = ConversationRuntime::builder(scripted(model), Tools::new().with(Renders))
        .system("S")
        .build();

    let turn = runtime.run_turn("go").await.unwrap();

    let output = &turn.tool_calls[0].output.content;
    assert_eq!(output, "System prompt:\nS\n\nTools: t\n\nUser:\ngo");
    let at = |part: &str| output.find(part).unwrap();
    assert!(at("S") < at("t") && at("t") < at("go"), "{output}");
}

/// E: adding a tool under a name already present panics naming it.
#[test]
#[should_panic(expected = "a tool named \"read\" is already in the set")]
fn duplicate_tool_name_panics() {
    let _ = read_and_write().with(Fixed {
        name: "read",
        reply: "again",
    });
}

/// C2(a): `cancelled` resolves at once on a fired token. (Adapted: the
/// context holds the token, not an `Option<&_>`.)
#[tokio::test]
async fn cancelled_resolves_on_a_fired_token() {
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let context = CallContext {
        cancel: token,
        ..CallContext::default()
    };

    let resolved = tokio::time::timeout(Duration::from_millis(50), context.cancelled()).await;

    assert!(resolved.is_ok(), "cancelled() resolves within 50 ms");
}

/// P3: a default context's `cancelled()` raced against a 50 ms timer loses.
#[tokio::test]
async fn p3_default_cancelled_loses_to_a_timer() {
    let context = CallContext::default();

    let winner = tokio::select! {
        () = context.cancelled() => "cancel",
        () = tokio::time::sleep(Duration::from_millis(50)) => "timer",
    };

    assert_eq!(winner, "timer");
}
