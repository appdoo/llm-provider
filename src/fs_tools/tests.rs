//! Tests shared by every file tool: typed input, derived schemas, the sandbox.

use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::agent::Tool;

/// A temp directory holding the sandbox root `d`.
fn sandbox_dir() -> (TempDir, Sandbox) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    let sandbox = Sandbox::new(dir.path().join("d")).unwrap();
    (dir, sandbox)
}

/// C-1: an input without `path` is an error naming the missing field.
#[tokio::test]
async fn read_without_path_is_invalid_input() {
    let (_dir, sandbox) = sandbox_dir();

    let out = ReadTool::new(sandbox).call(json!({})).await;

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

    let definition = GrepTool::new(sandbox).definition();

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
            Box::new(ReadTool::new(sandbox.clone())),
            json!({ "path": "../outside" }),
        ),
        (
            Box::new(WriteTool::new(sandbox.clone())),
            json!({ "path": "../outside", "content": "x" }),
        ),
        (
            Box::new(EditTool::new(sandbox.clone())),
            json!({ "path": "../outside", "old_string": "a", "new_string": "b" }),
        ),
        (
            Box::new(GrepTool::new(sandbox.clone())),
            json!({ "pattern": "x", "path": "../outside" }),
        ),
        (
            Box::new(GlobTool::new(sandbox)),
            json!({ "pattern": "*", "path": "../outside" }),
        ),
    ];

    for (tool, input) in calls {
        let out = tool.call(input).await;
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
