//! The Edit tool: replaces a string that occurs exactly once in a file.
//!
//! The new content goes to a temporary file in the same directory, which is
//! then renamed over the original, so a reader sees either the old or the
//! new file, never a half-written one.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

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
        let _ = (&self.sandbox, input);
        todo!()
    }
}

#[cfg(test)]
mod tests;
