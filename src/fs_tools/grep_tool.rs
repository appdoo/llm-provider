//! Grep tool — regex search across files.
//!
//! Uses `tokio::fs` for per-file reads (non-blocking I/O). Directory traversal
//! uses `std::fs::read_dir` synchronously — tokio's directory walker is built
//! on the same blocking primitive and the added overhead wouldn't buy anything
//! for this tool's workloads. When sandboxed, paths are resolved through the
//! [`Sandbox`] before any I/O.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;

use super::sandbox::Sandbox;
use crate::agent::tool::{optional_u64_clamped, require_string_nonempty, ToolExecutor, ToolOutput};
use crate::agent::ToolError;

/// Built-in Grep tool: searches file contents using regex.
pub struct GrepTool {
    sandbox: Option<Sandbox>,
}

impl GrepTool {
    pub fn new() -> Self {
        Self { sandbox: None }
    }

    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }

    /// Recursively walk a directory, skipping hidden entries.
    fn walk_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip hidden files and directories.
            if name_str.starts_with('.') {
                continue;
            }

            // `file_type()` does not follow links: symlinks are skipped.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                Self::walk_files(&path, files);
            } else if file_type.is_file() {
                files.push(path);
            }
        }
    }

    /// Search a single file for regex matches, returning formatted output lines.
    async fn search_file(path: &Path, re: &Regex, context: usize, base_dir: &Path) -> Vec<String> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return Vec::new(), // skip binary/unreadable files
        };

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        // Find all matching line indices.
        let match_indices: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, line)| re.is_match(line))
            .map(|(i, _)| i)
            .collect();

        if match_indices.is_empty() {
            return Vec::new();
        }

        // Build set of lines to include (matches + context).
        let mut include = vec![false; total];
        for &idx in &match_indices {
            let start = idx.saturating_sub(context);
            let end = (idx + context + 1).min(total);
            for slot in include.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }

        let rel_path = path.strip_prefix(base_dir).unwrap_or(path);

        let mut result = Vec::new();
        let mut prev_included = false;

        for (i, line) in lines.iter().enumerate() {
            if include[i] {
                if !prev_included && !result.is_empty() {
                    result.push("--".to_string());
                }
                let marker = if match_indices.contains(&i) { ":" } else { "-" };
                result.push(format!(
                    "{}{}{}{}",
                    rel_path.display(),
                    marker,
                    i + 1,
                    format_args!("{marker}{line}")
                ));
                prev_included = true;
            } else {
                prev_included = false;
            }
        }

        result
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_search_path(sandbox: &Option<Sandbox>, raw: &str) -> Result<PathBuf, ToolError> {
    match sandbox {
        Some(s) => s.resolve(raw),
        None => Ok(PathBuf::from(raw)),
    }
}

#[async_trait]
impl ToolExecutor for GrepTool {
    async fn execute(&self, input: serde_json::Value) -> ToolOutput {
        let pattern = match require_string_nonempty(&input, "pattern") {
            Ok(p) => p.to_string(),
            Err(_) => {
                return ToolOutput {
                    content: "Error: missing required parameter 'pattern'".to_string(),
                    is_error: true,
                };
            }
        };

        let re = match Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => {
                return ToolOutput {
                    content: format!("Invalid regex '{pattern}': {e}"),
                    is_error: true,
                };
            }
        };

        let raw_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".")
            .to_string();
        let search_path = match resolve_search_path(&self.sandbox, &raw_path) {
            Ok(p) => p,
            Err(e) => {
                return ToolOutput {
                    content: format!("Error: {e}"),
                    is_error: true,
                };
            }
        };

        // Clamp context to a sane ceiling — grep doesn't promise anything about
        // huge values and an LLM mis-specifying `context: 1_000_000` shouldn't
        // allocate a gigabyte of include bitmap.
        let context = optional_u64_clamped(&input, "context", 0, 0, 10_000) as usize;

        // Optional glob filter — not yet applied in the walk; preserved for future work.
        let _glob_filter = input.get("glob").and_then(|v| v.as_str());

        let mut files: Vec<PathBuf> = Vec::new();
        if search_path.is_file() {
            files.push(search_path.clone());
        } else if search_path.is_dir() {
            Self::walk_files(&search_path, &mut files);
        } else {
            return ToolOutput {
                content: format!("Path not found: {}", search_path.display()),
                is_error: true,
            };
        }

        files.sort();

        let base_dir: PathBuf = if search_path.is_dir() {
            search_path.clone()
        } else {
            search_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        };

        let mut all_results = Vec::new();
        for file in &files {
            let results = Self::search_file(file, &re, context, &base_dir).await;
            all_results.extend(results);
        }

        if all_results.is_empty() {
            ToolOutput {
                content: "No matches found".to_string(),
                is_error: false,
            }
        } else {
            ToolOutput {
                content: all_results.join("\n"),
                is_error: false,
            }
        }
    }

    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents with regex"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "File or directory to search" },
                "glob": { "type": "string", "description": "Glob pattern to filter files" },
                "context": { "type": "integer", "description": "Number of context lines around matches" }
            },
            "required": ["pattern"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn grep_no_sandbox_finds_matches() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("f.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let tool = GrepTool::new();
        let out = tool
            .execute(serde_json::json!({
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
        let tool = GrepTool::with_sandbox(sandbox);
        let out = tool
            .execute(serde_json::json!({
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
        let tool = GrepTool::with_sandbox(Sandbox::new(root).unwrap());
        tool.execute(serde_json::json!({ "pattern": "needle", "path": "." }))
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
}
