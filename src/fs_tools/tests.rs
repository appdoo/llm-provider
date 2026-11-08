//! Tests shared by every file tool: the `Files` block, typed input, derived
//! schemas, the sandbox.

use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::agent::{IntoTools, Tool, Toolbox, Tools, Transcript};

/// A temp directory holding the sandbox root `d`.
fn sandbox_dir() -> (TempDir, Sandbox) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    let sandbox = Sandbox::new(dir.path().join("d")).unwrap();
    (dir, sandbox)
}

/// The tool called `name` of a [`Files`] block over `sandbox`.
pub(super) fn file_tool(sandbox: Sandbox, name: &str) -> Box<dyn Tool> {
    let mut tools = Files::new(sandbox).into_tools();
    let index = tools
        .iter()
        .position(|tool| tool.definition().name == name)
        .unwrap();
    tools.swap_remove(index)
}

/// The names of the tools in `tools`, in order.
fn names(tools: &Tools) -> Vec<String> {
    tools.definitions().into_iter().map(|d| d.name).collect()
}

/// T-3: `Files` adds its five tools in source order; `only` keeps the named
/// ones in that order.
#[test]
fn files_adds_five_tools_and_only_keeps_the_named() {
    let (_dir, sandbox) = sandbox_dir();

    let all = Tools::new().with_all(Files::new(sandbox.clone()));
    let some = Tools::new()
        .with_all(Files::new(sandbox))
        .only(["edit", "read"]);

    assert_eq!(names(&all), ["read", "write", "edit", "grep", "glob"]);
    assert_eq!(names(&some), ["read", "edit"]);
}

/// C-1: an input without `path` is an error naming the missing field.
#[tokio::test]
async fn read_without_path_is_invalid_input() {
    let (_dir, sandbox) = sandbox_dir();

    let out = file_tool(sandbox, "read")
        .call(json!({}), &Transcript::default())
        .await;

    assert!(out.is_error);
    assert!(
        out.content.starts_with("invalid input: "),
        "{}",
        out.content
    );
    assert!(out.content.contains("path"), "{}", out.content);
}

/// C-5: Grep's schema is derived from its input: `pattern` and `path`, no
/// `glob`.
#[test]
fn grep_schema_has_pattern_and_path_without_glob() {
    let (_dir, sandbox) = sandbox_dir();

    let definition = file_tool(sandbox, "grep").definition();

    let properties = &definition.input_schema["properties"];
    assert!(properties.get("pattern").is_some(), "{properties}");
    assert!(properties.get("path").is_some(), "{properties}");
    assert!(properties.get("glob").is_none(), "{properties}");
    assert_eq!(definition.input_schema["required"], json!(["pattern"]));
}

/// C-6: every tool rejects `../outside` with the sandbox message.
#[tokio::test]
async fn every_tool_rejects_a_path_outside_the_sandbox() {
    let (_dir, sandbox) = sandbox_dir();
    let calls: Vec<(Box<dyn Tool>, serde_json::Value)> = vec![
        (
            file_tool(sandbox.clone(), "read"),
            json!({ "path": "../outside" }),
        ),
        (
            file_tool(sandbox.clone(), "write"),
            json!({ "path": "../outside", "content": "x" }),
        ),
        (
            file_tool(sandbox.clone(), "edit"),
            json!({ "path": "../outside", "old_string": "a", "new_string": "b" }),
        ),
        (
            file_tool(sandbox.clone(), "grep"),
            json!({ "pattern": "x", "path": "../outside" }),
        ),
        (
            file_tool(sandbox, "glob"),
            json!({ "pattern": "*", "path": "../outside" }),
        ),
    ];

    for (tool, input) in calls {
        let out = tool.call(input, &Transcript::default()).await;
        let name = tool.definition().name;
        assert!(out.is_error, "{name}: {}", out.content);
        assert!(
            out.content.starts_with("Sandbox error: "),
            "{name}: {}",
            out.content
        );
        assert!(
            out.content.contains("escapes sandbox root"),
            "{name}: {}",
            out.content
        );
    }
}
