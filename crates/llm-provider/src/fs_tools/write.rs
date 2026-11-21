//! The Write tool: creates or overwrites a file, creating its parent
//! directories.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Sandbox, ToolError};

/// The input of the Write tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WriteInput {
    /// File path to write.
    path: String,
    /// Content to write.
    content: String,
}

/// Runs the Write tool on `input`, inside `sandbox`.
pub(super) fn run(sandbox: &Sandbox, input: &WriteInput) -> Result<String, ToolError> {
    let path = sandbox.resolve(&input.path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ToolError::io(parent))?;
    }

    std::fs::write(&path, input.content.as_bytes()).map_err(ToolError::io(&path))?;
    Ok(format!(
        "Wrote {} bytes to {}",
        input.content.len(),
        input.path
    ))
}

#[cfg(test)]
mod tests;
