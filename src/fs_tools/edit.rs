//! The Edit tool: replaces a string that occurs exactly once in a file.
//!
//! The new content goes to a temporary file in the same directory, which is
//! then renamed over the original, so a reader sees either the old or the
//! new file, never a half-written one.

use std::path::Path;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tempfile::NamedTempFile;

use super::{FileTool, Sandbox, ToolError};

/// The Edit tool: replaces a unique string in a file inside its sandbox.
#[derive(Debug, Clone)]
pub struct EditTool {
    sandbox: Sandbox,
}

/// The input of the Edit tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct EditInput {
    /// File path to edit.
    path: String,
    /// Exact string to find (must be unique in the file).
    old_string: String,
    /// Replacement string.
    new_string: String,
}

impl EditTool {
    /// An Edit tool confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::fs_tools::{EditTool, Sandbox};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let edit = EditTool::new(Sandbox::new("docs")?);
    /// # let _ = edit;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl FileTool for EditTool {
    type Input = EditInput;
    const NAME: &'static str = "Edit";
    const DESCRIPTION: &'static str =
        "Replace an exact string in a file. The old_string must appear exactly once.";

    async fn run(&self, input: EditInput) -> Result<String, ToolError> {
        if input.old_string.is_empty() {
            let message = "old_string must not be empty";
            return Err(ToolError::InvalidInput(serde::de::Error::custom(message)));
        }
        let path = self.sandbox.resolve(&input.path)?;
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(ToolError::io(&path))?;
        if !meta.is_file() {
            return Err(ToolError::NotAFile(input.path));
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(ToolError::io(&path))?;
        let new_content = replace_once(&content, &input.old_string, &input.new_string, &path)?;

        write_atomically(&path, new_content).await?;
        Ok(format!("Edited {}", path.display()))
    }
}

/// `content` with its only occurrence of `old` replaced by `new`.
fn replace_once(content: &str, old: &str, new: &str, path: &Path) -> Result<String, ToolError> {
    match content.matches(old).count() {
        0 => Err(ToolError::NotFound(path.to_path_buf())),
        1 => Ok(content.replacen(old, new, 1)),
        count => Err(ToolError::NotUnique {
            count,
            path: path.to_path_buf(),
        }),
    }
}

/// Writes `content` to a temporary file beside `path`, then renames it over
/// `path`.
async fn write_atomically(path: &Path, content: String) -> Result<(), ToolError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temp = NamedTempFile::new_in(dir).map_err(ToolError::io(dir))?;

    tokio::fs::write(temp.path(), content)
        .await
        .map_err(ToolError::io(temp.path()))?;
    temp.persist(path)
        .map_err(|error| ToolError::io(path)(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
