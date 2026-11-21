use super::*;
use crate::agent::{CallContext, ToolOutput};
use crate::fs_tools::tests::file_tool;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::time::MissedTickBehavior;

#[tokio::test]
async fn grep_absolute_path_finds_matches() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("f.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "grep");
    let out = tool
        .call(
            serde_json::json!({
                "pattern": "fn main",
                "path": dir.path().to_str().unwrap()
            }),
            &CallContext::default(),
        )
        .await;
    assert!(!out.is_error);
    assert!(out.content.contains("fn main"));
}

#[tokio::test]
async fn grep_sandbox_rejects_escape() {
    let dir = TempDir::new().unwrap();
    let sandbox = Sandbox::new(dir.path()).unwrap();
    let tool = file_tool(sandbox, "grep");
    let out = tool
        .call(
            serde_json::json!({
                "pattern": "root",
                "path": "/etc"
            }),
            &CallContext::default(),
        )
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
    let tool = file_tool(Sandbox::new(root).unwrap(), "grep");
    tool.call(
        serde_json::json!({ "pattern": "needle", "path": "." }),
        &CallContext::default(),
    )
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
    let tool = file_tool(Sandbox::new(dir.path()).unwrap(), "grep");

    let out = tool
        .call(
            serde_json::json!({ "pattern": "fn", "path": "d" }),
            &CallContext::default(),
        )
        .await;

    let nested = Path::new("sub").join("y.rs");
    let expected = format!("{}:1:fn y() {{}}\nx.rs:1:fn x() {{}}", nested.display());
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(out.content, expected);
}

/// A temp tree of 40 000 files `d{i/1000}/f{i}.txt`, each `"x\n"`.
fn large_tree() -> TempDir {
    let dir = TempDir::new().unwrap();
    for i in 0..40_000 {
        let sub = dir.path().join(format!("d{}", i / 1000));
        if i % 1000 == 0 {
            std::fs::create_dir(&sub).unwrap();
        }
        std::fs::write(sub.join(format!("f{i}.txt")), "x\n").unwrap();
    }
    dir
}

/// The largest gap between consecutive `ticks` whose span overlaps
/// `start..end`.
fn largest_gap(ticks: &[Instant], start: Instant, end: Instant) -> Duration {
    ticks
        .windows(2)
        .filter(|pair| pair[1] >= start && pair[0] <= end)
        .map(|pair| pair[1] - pair[0])
        .max()
        .unwrap_or_default()
}

/// F1: a grep over a large tree leaves the one runtime thread free, so a
/// ticker on it never waits as long as half the walk.
#[tokio::test(flavor = "current_thread")]
async fn grep_over_a_large_tree_does_not_stall_the_runtime() {
    let dir = large_tree();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let timer = Instant::now();
    let _ = files_under(&root);
    let walk = timer.elapsed();
    assert!(
        walk >= Duration::from_millis(40),
        "precondition: the walk took {walk:?}, under 40 ms"
    );

    // A ticker on the same thread records when it gets to run.
    let ticks = Arc::new(Mutex::new(vec![Instant::now()]));
    let ticker = tokio::spawn({
        let ticks = Arc::clone(&ticks);
        async move {
            let mut interval = tokio::time::interval(Duration::from_millis(10));
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                ticks.lock().unwrap().push(Instant::now());
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;

    let tool = file_tool(Sandbox::new(&root).unwrap(), "grep");
    let start = Instant::now();
    let out = tool
        .call(
            serde_json::json!({ "pattern": "nomatch" }),
            &CallContext::default(),
        )
        .await;
    let end = Instant::now();
    tokio::time::sleep(Duration::from_millis(30)).await;
    ticker.abort();

    assert_eq!(out.content, "No matches found");
    let gap = largest_gap(&ticks.lock().unwrap(), start, end);
    assert!(
        gap < walk / 2,
        "largest tick gap {gap:?} during grep, walk {walk:?}"
    );
}
