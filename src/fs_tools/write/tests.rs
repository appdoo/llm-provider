use super::*;
use crate::fs_tools::tests::file_tool;
use tempfile::TempDir;

#[tokio::test]
async fn write_absolute_path_creates_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.txt");
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "write");
    let out = tool
        .call(serde_json::json!({
            "path": path.to_str().unwrap(),
            "content": "hi"
        }))
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi");
}

#[tokio::test]
async fn write_sandbox_rejects_escape() {
    let dir = TempDir::new().unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    let tool = file_tool(sandbox, "write");
    let out = tool
        .call(serde_json::json!({
            "path": "../../../tmp/escape.txt",
            "content": "nope"
        }))
        .await;
    assert!(out.is_error);
    assert!(out.content.contains("Sandbox"));
}

#[tokio::test]
async fn write_sandbox_accepts_in_root() {
    let dir = TempDir::new().unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    let tool = file_tool(sandbox, "write");
    let out = tool
        .call(serde_json::json!({ "path": "ok.txt", "content": "yes" }))
        .await;
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ok.txt")).unwrap(),
        "yes"
    );
}

// Write through a dangling symlink must not create the outside target.
#[cfg(unix)]
#[tokio::test]
async fn write_through_dangling_symlink_rejected() {
    use std::os::unix::fs::symlink;

    let root_dir = TempDir::new().unwrap();
    let outside_dir = TempDir::new().unwrap();
    let target = outside_dir.path().join("escaped.txt");
    symlink(&target, root_dir.path().join("link")).unwrap();

    let tool = file_tool(Sandbox::new(root_dir.path()).unwrap(), "write");
    let out = tool
        .call(serde_json::json!({ "path": "link", "content": "x" }))
        .await;
    assert!(out.is_error, "{}", out.content);
    assert!(!target.exists(), "write escaped the sandbox");
}
