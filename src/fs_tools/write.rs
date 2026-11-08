//! Write tool — create or overwrite a file with given content.
//!
//! Uses `tokio::fs` for non-blocking I/O. Optionally sandboxed via
//! [`Sandbox`] — every path goes through `Sandbox::resolve` before any
//! filesystem access when configured.

use std::path::PathBuf;

use async_trait::async_trait;

use super::sandbox::Sandbox;
use crate::agent::ToolError;
use crate::agent::{require_string, require_string_nonempty, Tool, ToolOutput};
use crate::provider::ToolDefinition;

/// Built-in Write tool: creates or overwrites files, creating parent directories as needed.
#[derive(Default)]
pub struct WriteTool {
    sandbox: Option<Sandbox>,
}

impl WriteTool {
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

fn resolve_path(sandbox: &Option<Sandbox>, path_arg: &str) -> Result<PathBuf, ToolError> {
    match sandbox {
        Some(s) => s.resolve(path_arg),
        None => Ok(PathBuf::from(path_arg)),
    }
}

#[async_trait]
impl Tool for WriteTool {
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
        let content = match require_string(&input, "content") {
            Ok(c) => c.to_string(),
            Err(_) => {
                return ToolOutput {
                    content: "Error: 'content' is required".to_string(),
                    is_error: true,
                };
            }
        };

        let file_path = match resolve_path(&self.sandbox, path_arg) {
            Ok(p) => p,
            Err(e) => {
                return ToolOutput {
                    content: format!("Error: {e}"),
                    is_error: true,
                };
            }
        };

        // Create parent directories if they don't exist.
        if let Some(parent) = file_path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    return ToolOutput {
                        content: format!("Error creating directories: {e}"),
                        is_error: true,
                    };
                }
            }
        }

        match tokio::fs::write(&file_path, content.as_bytes()).await {
            Ok(()) => ToolOutput {
                content: format!("Wrote {} bytes to {}", content.len(), path_arg),
                is_error: false,
            },
            Err(e) => ToolOutput {
                content: format!("Error writing {path_arg}: {e}"),
                is_error: true,
            },
        }
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Write".into(),
            description: "Create or overwrite a file. Creates parent directories automatically."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn write_no_sandbox_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("a.txt");
        let tool = WriteTool::default();
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
        let tool = WriteTool::with_sandbox(sandbox);
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
        let tool = WriteTool::with_sandbox(sandbox);
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

        let tool = WriteTool::with_sandbox(Sandbox::new(root_dir.path()).unwrap());
        let out = tool
            .call(serde_json::json!({ "path": "link", "content": "x" }))
            .await;
        assert!(out.is_error, "{}", out.content);
        assert!(!target.exists(), "write escaped the sandbox");
    }
}
