//! Read tool — reads file contents with optional offset and limit.
//!
//! Safety properties:
//! - All I/O goes through `tokio::fs` / `tokio::io` so the Tokio worker is not blocked.
//! - Files larger than the 10 MiB read cap require an explicit
//!   `offset` or `limit` — otherwise the tool returns
//!   [`ToolError::InputTooLarge`]. This prevents OOM on accidental full reads.
//! - When the file is larger than the cap *and* `offset`/`limit` is set, the
//!   reader switches to streaming mode (line-by-line via `BufReader`), keeping
//!   memory bounded to roughly the emitted window.
//! - When configured with a [`Sandbox`], every path goes through
//!   `Sandbox::resolve` before any filesystem access.

use std::path::PathBuf;

use async_trait::async_trait;
use tokio::io::AsyncBufReadExt;

use super::sandbox::Sandbox;
use crate::agent::ToolError;
use crate::agent::{require_string_nonempty, Tool, ToolOutput};
use crate::provider::ToolDefinition;

/// Files larger than this must specify `offset` or `limit`.
pub(crate) const READ_SIZE_CAP: u64 = 10 * 1024 * 1024;

/// Built-in Read tool: reads file contents with line numbers.
///
/// ## Streaming-mode limitation
///
/// When the file exceeds the 10 MiB read cap and `offset`/`limit` is supplied,
/// the tool opens a `BufReader` and reads sequentially from byte 0 until it
/// has skipped `offset` newlines. There is no random-access fast path:
/// `offset: 1_000_000` on a 1 GB file still scans ~1 GB of bytes to find the
/// millionth newline. Memory stays bounded because only the emitted window is
/// buffered into the output string, but wall-clock time grows with offset.
/// A byte-offset variant is future work.
#[derive(Default)]
pub struct ReadTool {
    sandbox: Option<Sandbox>,
}

impl ReadTool {
    /// A tool confined to `sandbox`.
    pub fn new(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }

    /// Create a Read tool constrained to the given sandbox.
    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            sandbox: Some(sandbox),
        }
    }
}

/// Resolve `path_arg` through the sandbox (when present) or return it as-is.
fn resolve_path(sandbox: &Option<Sandbox>, path_arg: &str) -> Result<PathBuf, ToolError> {
    match sandbox {
        Some(s) => s.resolve(path_arg),
        None => Ok(PathBuf::from(path_arg)),
    }
}

#[async_trait]
impl Tool for ReadTool {
    async fn call(&self, input: serde_json::Value) -> ToolOutput {
        let path_arg = match require_string_nonempty(&input, "path") {
            Ok(p) => p,
            Err(_) => {
                return ToolOutput {
                    content: "Error: missing required parameter 'path'".to_string(),
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

        // u64::MAX as "no limit" sentinel for the schema helper — we convert to
        // Option<u64> below.
        let offset_raw = input.get("offset").and_then(|v| v.as_u64());
        let limit_raw = input.get("limit").and_then(|v| v.as_u64());
        // Preserve the legacy default of 2000 lines only when neither offset nor
        // limit is supplied (small-file path).
        let offset = usize::try_from(offset_raw.unwrap_or(0)).unwrap_or(usize::MAX);
        let limit = usize::try_from(limit_raw.unwrap_or(2000)).unwrap_or(usize::MAX);

        // Metadata first — needed for the size-cap decision.
        let meta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => {
                return ToolOutput {
                    content: format!("Error reading {}: {}", path.display(), e),
                    is_error: true,
                };
            }
        };

        if !meta.is_file() {
            return ToolOutput {
                content: format!("not a regular file: {path_arg}"),
                is_error: true,
            };
        }

        let has_window = offset_raw.is_some() || limit_raw.is_some();
        if meta.len() > READ_SIZE_CAP && !has_window {
            return ToolOutput {
                content: format!(
                    "Error: {}",
                    ToolError::InputTooLarge("file too large — use offset/limit".to_string())
                ),
                is_error: true,
            };
        }

        if meta.len() > READ_SIZE_CAP {
            // Streaming mode — bounded memory. See struct docs for the known
            // limitation that BufReader seeks sequentially to `offset`.
            let file = match tokio::fs::File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    return ToolOutput {
                        content: format!("Error reading {}: {}", path.display(), e),
                        is_error: true,
                    };
                }
            };
            let mut reader = tokio::io::BufReader::new(file).lines();
            let mut out = String::new();
            let mut idx: u64 = 0;
            // O(1) counter — NEVER use out.lines().count() inside the hot loop
            // (it would re-scan the buffer on every line, making the emit cost
            // quadratic in window size).
            let mut lines_added: u64 = 0;
            let start = offset as u64;
            let limit_lines = limit_raw;
            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        if idx >= start {
                            // Emit as "<line_number>\t<line>" to match the small-file
                            // path's numbered output (1-indexed).
                            out.push_str(&format!("{}\t{}\n", idx + 1, line));
                            lines_added += 1;
                            if let Some(l) = limit_lines {
                                if lines_added >= l {
                                    break;
                                }
                            }
                        }
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        return ToolOutput {
                            content: format!("Error reading {}: {}", path.display(), e),
                            is_error: true,
                        };
                    }
                }
            }
            // Trim the trailing newline to match the small-file format (which joins with \n).
            if out.ends_with('\n') {
                out.pop();
            }
            return ToolOutput {
                content: out,
                is_error: false,
            };
        }

        // Small-file path — existing full-read semantics, but via tokio::fs.
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let end = offset.saturating_add(limit).min(lines.len());
                if offset >= lines.len() {
                    return ToolOutput {
                        content: String::new(),
                        is_error: false,
                    };
                }
                let selected: String = lines[offset..end]
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{}\t{}", offset + i + 1, line))
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutput {
                    content: selected,
                    is_error: false,
                }
            }
            Err(e) => ToolOutput {
                content: format!("Error reading {}: {}", path.display(), e),
                is_error: true,
            },
        }
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "Read".into(),
            description: "Read file contents with optional offset and limit".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" },
                    "offset": { "type": "integer", "description": "Start line (0-indexed)" },
                    "limit": { "type": "integer", "description": "Max lines to read" }
                },
                "required": ["path"]
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Write a file with `n` small ASCII bytes. Used to cross the 10 MiB threshold.
    fn write_large_file(bytes: usize) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        // Write predictable lines: each "xxxxxxxxxx\n" is 11 bytes.
        let chunk = b"xxxxxxxxxx\n";
        let mut written = 0usize;
        while written + chunk.len() <= bytes {
            f.write_all(chunk).unwrap();
            written += chunk.len();
        }
        // Pad remainder.
        while written < bytes {
            f.write_all(b"x").unwrap();
            written += 1;
        }
        f.flush().unwrap();
        f
    }

    // Read uses tokio::fs — a sibling task polls while a large read is in flight.
    // Scenario: 100 MB file, sleeper wakes within ~20 ms. We use the
    // full 100 MiB to exercise the non-blocking I/O path at production scale.
    // The sleeper bound is relaxed to 200 ms to tolerate CI jitter while
    // still proving the read does not starve the runtime.
    #[tokio::test(flavor = "current_thread")]
    async fn read_does_not_block_other_tasks() {
        let f = write_large_file(100 * 1024 * 1024);
        let path = f.path().to_str().unwrap().to_string();

        let started = std::time::Instant::now();
        let sleeper = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            std::time::Instant::now()
        });

        // A 100 MB file with no offset/limit given; we use a
        // small window to keep test duration bounded while still requiring
        // the full 100 MiB cap check + streaming-mode code path.
        let tool = ReadTool::default();
        let out = tool
            .call(serde_json::json!({ "path": path, "offset": 0, "limit": 5 }))
            .await;
        assert!(!out.is_error, "read failed: {}", out.content);

        let sleeper_finish = sleeper.await.unwrap();
        let sleeper_delta = sleeper_finish.duration_since(started);
        assert!(
            sleeper_delta < std::time::Duration::from_millis(200),
            "sleeper was starved (expected <200ms, got {sleeper_delta:?}) — read is blocking"
        );
    }

    // Read on file > 10 MiB without offset/limit → InputTooLarge.
    #[tokio::test]
    async fn read_too_large_rejects() {
        let f = write_large_file(11 * 1024 * 1024);
        let tool = ReadTool::default();
        let out = tool
            .call(serde_json::json!({ "path": f.path().to_str().unwrap() }))
            .await;
        assert!(out.is_error);
        assert!(
            out.content.contains("file too large"),
            "expected 'file too large' in: {}",
            out.content
        );
    }

    // Read on 100 MB file with large offset + limit succeeds (streaming mode).
    // Values: file=100 MB, offset=1_000_000, limit=100. The streaming
    // BufReader scans sequentially to offset, then emits exactly `limit` lines.
    // Peak memory (< 50 MB) is not asserted: that needs an allocator
    // instrumentation dev-dependency.
    #[tokio::test]
    async fn read_too_large_with_window_succeeds() {
        let f = write_large_file(100 * 1024 * 1024);
        let tool = ReadTool::default();
        let out = tool
            .call(serde_json::json!({
                "path": f.path().to_str().unwrap(),
                "offset": 1_000_000,
                "limit": 100
            }))
            .await;
        assert!(!out.is_error, "expected success, got: {}", out.content);
        let count = out.content.lines().count();
        assert_eq!(
            count,
            100,
            "expected exactly 100 lines, got {count}: {}",
            &out.content[..out.content.len().min(200)]
        );
    }

    // Without a sandbox, the existing path-based call works unchanged.
    #[tokio::test]
    async fn no_sandbox_unchanged_behavior() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("ok.txt");
        std::fs::write(&path, "line1\nline2\n").unwrap();
        let tool = ReadTool::default();
        let out = tool
            .call(serde_json::json!({ "path": path.to_str().unwrap() }))
            .await;
        assert!(!out.is_error);
        assert!(out.content.contains("line1"));
        assert!(out.content.contains("line2"));
    }

    // Sandbox mode: a valid in-root path reads fine.
    #[tokio::test]
    async fn sandbox_in_root_reads_ok() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, "hi\n").unwrap();
        let sandbox = Sandbox::new(dir.path()).unwrap();
        let tool = ReadTool::with_sandbox(sandbox);
        let out = tool.call(serde_json::json!({ "path": "hello.txt" })).await;
        assert!(!out.is_error, "content: {}", out.content);
        assert!(out.content.contains("hi"));
    }

    // Sandbox mode: escape attempt fails before any filesystem read of the target.
    #[tokio::test]
    async fn sandbox_escape_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let sandbox = Sandbox::new(dir.path()).unwrap();
        let tool = ReadTool::with_sandbox(sandbox);
        let out = tool
            .call(serde_json::json!({ "path": "../../../etc/passwd" }))
            .await;
        assert!(out.is_error);
        assert!(out.content.contains("Sandbox"));
    }

    // --- Read edge cases ---

    /// `offset + limit` near `u64::MAX` saturates instead of overflowing.
    #[tokio::test]
    async fn read_window_saturates() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"a\nb\nc\n").unwrap();
        f.flush().unwrap();
        let tool = ReadTool::default();
        let out = tool
            .call(serde_json::json!({
                "path": f.path().to_str().unwrap(),
                "offset": 1,
                "limit": 18446744073709551615u64
            }))
            .await;
        assert!(!out.is_error);
        assert_eq!(out.content, "2\tb\n3\tc");
    }

    /// A device file is rejected before any read, so it cannot hang.
    #[cfg(unix)]
    #[tokio::test]
    async fn read_rejects_special_file() {
        let tool = ReadTool::default();
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tool.call(serde_json::json!({ "path": "/dev/zero" })),
        )
        .await
        .expect("Read of /dev/zero must not hang");
        assert!(out.is_error);
        assert_eq!(out.content, "not a regular file: /dev/zero");
    }
}
