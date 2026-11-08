//! Glob tool — find files matching a glob pattern.
//!
//! When configured with a [`Sandbox`], the `path` base argument is resolved
//! through `Sandbox::resolve` before any matching happens. The glob pattern
//! itself is not resolved (it may contain wildcards), but any resulting path
//! is rechecked against the sandbox root and filtered out on escape.

use std::path::PathBuf;

use async_trait::async_trait;

use super::sandbox::Sandbox;
use crate::agent::ToolError;
use crate::agent::{require_string_nonempty, Tool, ToolOutput};
use crate::provider::ToolDefinition;

/// Built-in Glob tool: matches files using glob patterns.
pub struct GlobTool {
    sandbox: Option<Sandbox>,
}

impl GlobTool {
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

impl Default for GlobTool {
    fn default() -> Self {
        Self { sandbox: None }
    }
}

fn resolve_base(sandbox: &Option<Sandbox>, base: &str) -> Result<PathBuf, ToolError> {
    match sandbox {
        Some(s) => s.resolve(base),
        None => Ok(PathBuf::from(base)),
    }
}

#[async_trait]
impl Tool for GlobTool {
    async fn call(&self, input: serde_json::Value) -> ToolOutput {
        let pattern = match require_string_nonempty(&input, "pattern") {
            Ok(p) => p.to_string(),
            Err(_) => {
                return ToolOutput {
                    content: "Error: missing required parameter 'pattern'".to_string(),
                    is_error: true,
                };
            }
        };

        // If a path is given and pattern is relative, join them.
        let full_pattern = match input.get("path").and_then(|v| v.as_str()) {
            Some(base) => {
                if pattern.starts_with('/') {
                    // Absolute glob — base ignored; still honor the sandbox filter below.
                    pattern
                } else {
                    let base_path = match resolve_base(&self.sandbox, base) {
                        Ok(p) => p,
                        Err(e) => {
                            return ToolOutput {
                                content: format!("Error: {e}"),
                                is_error: true,
                            };
                        }
                    };
                    format!("{}/{}", base_path.display(), pattern)
                }
            }
            None => pattern,
        };

        match glob::glob(&full_pattern) {
            Ok(entries) => {
                let mut paths: Vec<std::path::PathBuf> = entries.filter_map(|e| e.ok()).collect();

                // If we have a sandbox, drop any match that escapes the root
                // (e.g. symlink targets). Canonicalize best-effort; on failure
                // the path is rejected.
                if let Some(s) = &self.sandbox {
                    paths.retain(|p| match std::fs::canonicalize(p) {
                        Ok(c) => c.starts_with(s.root()),
                        Err(_) => false,
                    });
                }

                // Sort by modification time (most recent first), fall back to path order.
                paths.sort_by(|a, b| {
                    let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
                    let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
                    mtime_b.cmp(&mtime_a)
                });

                let output = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput {
                    content: output,
                    is_error: false,
                }
            }
            Err(e) => ToolOutput {
                content: format!("Glob pattern error: {e}"),
                is_error: true,
            },
        }
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Glob".into(),
            description: "Find files matching a glob pattern".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match files" },
                    "path": { "type": "string", "description": "Base directory for relative patterns" }
                },
                "required": ["pattern"]
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn glob_no_sandbox_matches() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        let tool = GlobTool::default();
        let out = tool
            .call(serde_json::json!({
                "pattern": format!("{}/*.rs", dir.path().display())
            }))
            .await;
        assert!(!out.is_error);
        let lines: Vec<_> = out.content.lines().collect();
        assert_eq!(lines.len(), 2);
    }

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
        let tool = GlobTool::with_sandbox(sandbox);
        let out = tool
            .call(serde_json::json!({
                "pattern": "*.txt",
                "path": "."
            }))
            .await;
        assert!(!out.is_error, "{}", out.content);
        // Only ok.txt should remain (escape.txt canonicalizes outside root).
        assert!(out.content.contains("ok.txt"));
        assert!(!out.content.contains("escape.txt"));
    }
}
