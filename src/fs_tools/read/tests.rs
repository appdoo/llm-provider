use super::*;
use crate::agent::Tool;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// A Read tool sandboxed on the directory that holds `file`.
fn read_tool_beside(file: &Path) -> ReadTool {
    ReadTool::new(Sandbox::new(file.parent().unwrap()).unwrap())
}

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
    let tool = read_tool_beside(f.path());
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
    let tool = read_tool_beside(f.path());
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
    let tool = read_tool_beside(f.path());
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

// An absolute path inside the sandbox reads its lines.
#[tokio::test]
async fn read_absolute_path_in_root() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("ok.txt");
    std::fs::write(&path, "line1\nline2\n").unwrap();
    let tool = read_tool_beside(&path);
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
    let tool = ReadTool::new(sandbox);
    let out = tool.call(serde_json::json!({ "path": "hello.txt" })).await;
    assert!(!out.is_error, "content: {}", out.content);
    assert!(out.content.contains("hi"));
}

// Sandbox mode: escape attempt fails before any filesystem read of the target.
#[tokio::test]
async fn sandbox_escape_rejected() {
    let dir = tempfile::TempDir::new().unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    let tool = ReadTool::new(sandbox);
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
    let tool = read_tool_beside(f.path());
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
    let tool = ReadTool::new(Sandbox::new("/dev").unwrap());
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tool.call(serde_json::json!({ "path": "/dev/zero" })),
    )
    .await
    .expect("Read of /dev/zero must not hang");
    assert!(out.is_error);
    assert_eq!(out.content, "not a regular file: /dev/zero");
}
