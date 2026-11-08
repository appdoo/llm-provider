//! Edit tool — replace an exact unique string in a file.
//!
//! Writes are atomic on POSIX: the tool reads the file, computes the new
//! content, writes it to a sibling temp file with a random UUID suffix, then
//! issues a single `rename(2)` to swap it into place. Concurrent readers see
//! either the fully-old or the fully-new content, never a half-written state.
//! Windows is out of scope.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::sandbox::Sandbox;
use crate::agent::ToolError;
use crate::agent::{require_string, require_string_nonempty, Tool, ToolOutput};
use crate::provider::ToolDefinition;

/// Built-in Edit tool: finds and replaces an exact string that must appear exactly once.
pub struct EditTool {
    sandbox: Option<Sandbox>,
}

impl EditTool {
    /// A tool confined to `sandbox`.
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }
}

impl Default for EditTool {
    fn default() -> Self {
        Self { sandbox: None }
    }
}

fn resolve_path(sandbox: &Option<Sandbox>, path_arg: &str) -> Result<PathBuf, ToolError> {
    match sandbox {
        Some(s) => s.resolve(path_arg),
        None => Ok(PathBuf::from(path_arg)),
    }
}

/// Build an atomic tmp path: `{file}.tmp.{uuid}`, always in the same directory
/// so `rename(2)` stays on the same filesystem.
fn tmp_path(target: &Path) -> PathBuf {
    let uuid = uuid::Uuid::new_v4();
    let file_name = target
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    let mut tmp_name = file_name;
    tmp_name.push(format!(".tmp.{uuid}"));
    match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    }
}

#[async_trait]
impl Tool for EditTool {
    async fn call(&self, input: serde_json::Value) -> ToolOutput {
        let path_arg = match require_string_nonempty(&input, "path") {
            Ok(p) => p,
            Err(_) => {
                return ToolOutput {
                    content: "Error: 'path' is required".to_string(),
                    is_error: true,
                };
            }
        };
        let old_string = match require_string_nonempty(&input, "old_string") {
            Ok(s) => s.to_string(),
            Err(_) => {
                return ToolOutput {
                    content: "Error: 'old_string' is required and must not be empty".to_string(),
                    is_error: true,
                };
            }
        };
        let new_string = match require_string(&input, "new_string") {
            Ok(s) => s.to_string(),
            Err(_) => {
                return ToolOutput {
                    content: "Error: 'new_string' is required".to_string(),
                    is_error: true,
                };
            }
        };

        let path = match resolve_path(&self.sandbox, path_arg) {
            Ok(p) => p,
            Err(e) => {
                return ToolOutput {
                    content: format!("Error: {e}"),
                    is_error: true,
                };
            }
        };

        match tokio::fs::metadata(&path).await {
            Ok(m) if m.is_file() => {}
            Ok(_) => {
                return ToolOutput {
                    content: format!("not a regular file: {path_arg}"),
                    is_error: true,
                };
            }
            Err(e) => {
                return ToolOutput {
                    content: format!("Error reading {}: {}", path.display(), e),
                    is_error: true,
                };
            }
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => {
                return ToolOutput {
                    content: format!("Error reading {}: {}", path.display(), e),
                    is_error: true,
                };
            }
        };

        let count = content.matches(&old_string).count();
        match count {
            0 => ToolOutput {
                content: format!("Error: string not found in {}", path.display()),
                is_error: true,
            },
            1 => {
                let new_content = content.replacen(&old_string, &new_string, 1);
                // Atomic write: tmp → rename. See module docs.
                let tmp = tmp_path(&path);
                if let Err(e) = tokio::fs::write(&tmp, new_content.as_bytes()).await {
                    // Best-effort cleanup (ignore errors — tmp may not exist).
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return ToolOutput {
                        content: format!("Error writing {}: {}", path.display(), e),
                        is_error: true,
                    };
                }
                if let Err(e) = tokio::fs::rename(&tmp, &path).await {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return ToolOutput {
                        content: format!("Error renaming into {}: {}", path.display(), e),
                        is_error: true,
                    };
                }
                ToolOutput {
                    content: format!("Edited {}", path.display()),
                    is_error: false,
                }
            }
            n => ToolOutput {
                content: format!(
                    "Error: string appears {} times in {} — provide more context to make it unique",
                    n,
                    path.display()
                ),
                is_error: true,
            },
        }
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Edit".into(),
            description:
                "Replace an exact string in a file. The old_string must appear exactly once.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to edit" },
                    "old_string": { "type": "string", "description": "Exact string to find (must be unique in the file)" },
                    "new_string": { "type": "string", "description": "Replacement string" }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let tool = Arc::new(EditTool::default());
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
                .call(serde_json::json!({
                    "path": p_for_edit.to_str().unwrap(),
                    "old_string": "OLD_MARKER",
                    "new_string": "NEW_MARKER"
                }))
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
        let tool = EditTool::default();
        let out = tool
            .call(serde_json::json!({
                "path": path.to_str().unwrap(),
                "old_string": "hello",
                "new_string": "bye"
            }))
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
        let tool = EditTool::default();
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tool.call(serde_json::json!({
                "path": "/dev/zero",
                "old_string": "a",
                "new_string": "b"
            })),
        )
        .await
        .expect("Edit of /dev/zero must not hang");
        assert!(out.is_error);
        assert_eq!(out.content, "not a regular file: /dev/zero");
    }
}
