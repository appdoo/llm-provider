//! The relay of a spawned server's stderr to the caller's stderr.

use std::io::Write;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// Copies `stderr` to the caller's stderr line by line, each line prefixed
/// `[<label>] `, until the server closes it.
pub(crate) fn relay(stderr: impl AsyncRead + Unpin + Send + 'static, label: String) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            // A closed or full stderr of the caller ends nothing; the line is lost.
            let _ = writeln!(std::io::stderr().lock(), "[{label}] {line}");
        }
    });
}
