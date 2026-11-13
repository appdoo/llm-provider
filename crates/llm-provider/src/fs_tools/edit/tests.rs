use super::*;
use crate::agent::Transcript;
use crate::fs_tools::tests::file_tool;
use std::sync::Arc;
use tempfile::TempDir;

// 100 concurrent reads during an edit — every read returns either
// the fully-old content or the fully-new content, never partial.
//
// Setup: a 1 MiB file containing the pattern "OLD_MARKER" exactly once.
// An Edit task replaces it with "NEW_MARKER". In parallel, 100 reads hit
// the same path. Each read must match exactly one of the two full snapshots.
#[tokio::test]
async fn edit_atomicity_concurrent_reads() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.txt");

    // Build ~1 MiB content: 1024 copies of a 1 KiB block, with OLD_MARKER in the middle.
    let block = "a".repeat(1024);
    let mut content = String::with_capacity(1024 * 1024 + 32);
    for _ in 0..512 {
        content.push_str(&block);
    }
    content.push_str("OLD_MARKER");
    for _ in 0..512 {
        content.push_str(&block);
    }
    tokio::fs::write(&path, &content).await.unwrap();
    let old_snapshot = content.clone();
    let new_snapshot = content.replacen("OLD_MARKER", "NEW_MARKER", 1);

    let tool = Arc::new(file_tool(Sandbox::new(dir.path()).unwrap(), "edit"));
    let path_arc = Arc::new(path.clone());

    // Spawn 100 readers first. They all race the edit.
    let mut readers = Vec::with_capacity(100);
    for _ in 0..100 {
        let p = Arc::clone(&path_arc);
        readers.push(tokio::spawn(
            async move { tokio::fs::read_to_string(&*p).await },
        ));
    }

    // Kick the edit at the same time.
    let tool_clone = Arc::clone(&tool);
    let p_for_edit = path.clone();
    let editor = tokio::spawn(async move {
        tool_clone
            .call(
                serde_json::json!({
                    "path": p_for_edit.to_str().unwrap(),
                    "old_string": "OLD_MARKER",
                    "new_string": "NEW_MARKER"
                }),
                &Transcript::default(),
            )
            .await
    });

    let edit_out = editor.await.unwrap();
    assert!(!edit_out.is_error, "edit failed: {}", edit_out.content);

    for (i, r) in readers.into_iter().enumerate() {
        let got = r.await.unwrap().expect("reader errored");
        assert!(
            got == old_snapshot || got == new_snapshot,
            "reader {i} saw partial content (len {}, expected {} or {})",
            got.len(),
            old_snapshot.len(),
            new_snapshot.len()
        );
    }

    // Final content must be the new snapshot.
    let final_content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(final_content, new_snapshot);
}

// tmp file does not linger after a successful edit.
#[tokio::test]
async fn edit_cleans_up_tmp_on_success() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("x.txt");
    tokio::fs::write(&path, "hello world").await.unwrap();
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "edit");
    let out = tool
        .call(
            serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_string": "hello",
                "new_string": "bye"
            }),
            &Transcript::default(),
        )
        .await;
    assert!(!out.is_error, "{}", out.content);
    // Verify no .tmp.* siblings in the dir.
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    while let Some(e) = entries.next_entry().await.unwrap() {
        let name = e.file_name();
        let n = name.to_string_lossy();
        assert!(!n.contains(".tmp."), "leftover tmp file: {n}");
    }
}

// --- Device files ---

/// Edit rejects a device file before reading it.
#[cfg(unix)]
#[tokio::test]
async fn edit_rejects_special_file() {
    let tool = file_tool(Sandbox::new("/dev").unwrap(), "edit");
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tool.call(
            serde_json::json!({
                "path": "/dev/zero",
                "old_string": "a",
                "new_string": "b"
            }),
            &Transcript::default(),
        ),
    )
    .await
    .expect("Edit of /dev/zero must not hang");
    assert!(out.is_error);
    assert_eq!(out.content, "not a regular file: /dev/zero");
}

/// C-3: the edit lands in `a.txt` and leaves no other file behind.
#[tokio::test]
async fn edit_replaces_and_leaves_no_temp_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "old").unwrap();
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "edit");

    let out = tool
        .call(
            serde_json::json!({ "path": "a.txt", "old_string": "old", "new_string": "new" }),
            &Transcript::default(),
        )
        .await;

    assert!(!out.is_error, "{}", out.content);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "new"
    );
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.txt"]);
}

// --- Permission bits ---

/// Writes `a.txt` with `mode`, edits it, and returns the new content and mode bits.
#[cfg(unix)]
async fn edit_file_with_mode(mode: u32) -> (String, u32) {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "old").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "edit");

    let out = tool
        .call(
            serde_json::json!({ "path": "a.txt", "old_string": "old", "new_string": "new" }),
            &Transcript::default(),
        )
        .await;

    assert!(!out.is_error, "{}", out.content);
    let content = std::fs::read_to_string(&path).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    (content, mode)
}

/// E-1: an executable file keeps mode 0755 after an edit.
#[cfg(unix)]
#[tokio::test]
async fn edit_keeps_executable_mode() {
    let (content, mode) = edit_file_with_mode(0o755).await;

    assert_eq!(content, "new");
    assert_eq!(mode, 0o755, "mode was {mode:o}");
}

/// E-2: a regular file keeps mode 0644 after an edit.
#[cfg(unix)]
#[tokio::test]
async fn edit_keeps_regular_mode() {
    let (content, mode) = edit_file_with_mode(0o644).await;

    assert_eq!(content, "new");
    assert_eq!(mode, 0o644, "mode was {mode:o}");
}
