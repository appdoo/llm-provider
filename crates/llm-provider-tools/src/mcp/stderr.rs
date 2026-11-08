//! The relay of a spawned server's stderr to the caller's stderr.

use tokio::io::AsyncRead;

/// Copies `stderr` to the caller's stderr line by line, each line prefixed
/// `[<label>] `, until the server closes it.
pub(crate) fn relay(stderr: impl AsyncRead + Unpin + Send + 'static, label: String) {
    let _ = (stderr, label);
    todo!()
}
