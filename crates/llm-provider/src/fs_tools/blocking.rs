//! [`blocking`]: file work run off the async runtime.

use super::ToolError;

/// Runs the blocking file work `work` on tokio's blocking pool; a panic in
/// it is [`ToolError::Panicked`].
pub(super) async fn blocking<T, F>(work: F) -> Result<T, ToolError>
where
    F: FnOnce() -> Result<T, ToolError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|join| Err(ToolError::Panicked(join)))
}

#[cfg(test)]
mod tests;
