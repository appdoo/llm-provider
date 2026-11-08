use super::*;
use crate::agent::{Tool, ToolOutput};
use std::path::Path;
use tempfile::TempDir;

#[tokio::test]
async fn grep_absolute_path_finds_matches() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("f.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let tool = GrepTool::new(Sandbox::new(dir.path()).unwrap());
    let out = tool
        .call(serde_json::json!({
            "pattern": "fn main",
            "path": dir.path().to_str().unwrap()
        }))
        .await;
    assert!(!out.is_error);
    assert!(out.content.contains("fn main"));
}

#[tokio::test]
async fn grep_sandbox_rejects_escape() {
    let dir = TempDir::new().unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    let tool = GrepTool::new(sandbox);
    let out = tool
        .call(serde_json::json!({
            "pattern": "root",
            "path": "/etc"
        }))
        .await;
    assert!(out.is_error);
    assert!(out.content.contains("Sandbox") || out.content.contains("Error"));
}

// --- Grep skips symlinks ---

/// Root `R` with `a.txt`, outside dir `O` with `b.txt`, and `R/out -> O`.
#[cfg(unix)]
fn symlink_fixture() -> (TempDir, TempDir) {
    let r = TempDir::new().unwrap();
    let o = TempDir::new().unwrap();
    std::fs::write(r.path().join("a.txt"), "needle\n").unwrap();
    std::fs::write(o.path().join("b.txt"), "needle\n").unwrap();
    std::os::unix::fs::symlink(o.path(), r.path().join("out")).unwrap();
    (r, o)
}

#[cfg(unix)]
async fn grep_needle(root: &Path) -> ToolOutput {
    let tool = GrepTool::new(Sandbox::new(root).unwrap());
    tool.call(serde_json::json!({ "pattern": "needle", "path": "." }))
        .await
}

/// A directory symlink pointing outside the root is not walked.
#[cfg(unix)]
#[tokio::test]
async fn grep_skips_directory_symlink() {
    let (r, _o) = symlink_fixture();
    let out = grep_needle(r.path()).await;
    assert!(!out.is_error);
    assert_eq!(out.content, "a.txt:1:needle");
}

/// A file symlink pointing outside the root is not read.
#[cfg(unix)]
#[tokio::test]
async fn grep_skips_file_symlink() {
    let (r, o) = symlink_fixture();
    std::os::unix::fs::symlink(o.path().join("b.txt"), r.path().join("f")).unwrap();
    let out = grep_needle(r.path()).await;
    assert_eq!(out.content, "a.txt:1:needle");
}

/// A symlink loop back to the root is not followed.
#[cfg(unix)]
#[tokio::test]
async fn grep_skips_symlink_loop() {
    let (r, _o) = symlink_fixture();
    std::os::unix::fs::symlink(r.path(), r.path().join("loop")).unwrap();
    let out = grep_needle(r.path()).await;
    assert_eq!(out.content, "a.txt:1:needle");
}

/// C-4: the walk reaches files in subdirectories.
#[tokio::test]
async fn grep_walks_subdirectories() {
    let dir = TempDir::new().unwrap();
    let d = dir.path().join("d");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(d.join("x.rs"), "fn x() {}\n").unwrap();
    std::fs::write(d.join("sub").join("y.rs"), "fn y() {}\n").unwrap();
    let tool = GrepTool::new(Sandbox::new(dir.path()).unwrap());

    let out = tool
        .call(serde_json::json!({ "pattern": "fn", "path": "d" }))
        .await;

    let nested = Path::new("sub").join("y.rs");
    let expected = format!("{}:1:fn y() {{}}\nx.rs:1:fn x() {{}}", nested.display());
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(out.content, expected);
}
