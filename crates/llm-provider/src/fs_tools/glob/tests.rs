use super::*;
use crate::agent::CallContext;
use crate::fs_tools::tests::file_tool;
use tempfile::TempDir;

#[tokio::test]
async fn glob_absolute_pattern_matches() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.rs"), "").unwrap();
    std::fs::write(dir.path().join("b.rs"), "").unwrap();
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "glob");
    let out = tool
        .call(
            serde_json::json!({
                "pattern": format!("{}/*.rs", dir.path().display())
            }),
            &CallContext::default(),
        )
        .await;
    assert!(!out.is_error);
    let lines: Vec<_> = out.content.lines().collect();
    assert_eq!(lines.len(), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn glob_sandbox_filters_escapes() {
    // Sandbox root has one .txt file; a symlink inside points outside and
    // must be filtered out of glob results.
    use std::os::unix::fs::symlink;
    let root_dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let outside_file = outside.path().join("secret.txt");
    std::fs::write(&outside_file, "secret").unwrap();

    let real_file = root_dir.path().join("ok.txt");
    std::fs::write(&real_file, "ok").unwrap();
    let link = root_dir.path().join("escape.txt");
    symlink(&outside_file, &link).unwrap();

    let sandbox = Sandbox::new(root_dir.path()).unwrap();
    let tool = file_tool(sandbox, "glob");
    let out = tool
        .call(
            serde_json::json!({
                "pattern": "*.txt",
                "path": "."
            }),
            &CallContext::default(),
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    // Only ok.txt should remain (escape.txt canonicalizes outside root).
    assert!(out.content.contains("ok.txt"));
    assert!(!out.content.contains("escape.txt"));
}
