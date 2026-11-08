//! The Write tool: creates or overwrites a file, creating its parent
//! directories.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{FileTool, Sandbox, ToolError};

/// The Write tool: writes a file inside its sandbox.
#[derive(Debug, Clone)]
pub struct WriteTool {
    sandbox: Sandbox,
}

/// The input of the Write tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WriteInput {
    /// File path to write.
    path: String,
    /// Content to write.
    content: String,
}

impl WriteTool {
    /// A Write tool confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::fs_tools::{Sandbox, WriteTool};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let write = WriteTool::new(Sandbox::new("docs")?);
    /// # let _ = write;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl FileTool for WriteTool {
    type Input = WriteInput;
    const NAME: &'static str = "Write";
    const DESCRIPTION: &'static str =
        "Create or overwrite a file. Creates parent directories automatically.";

    async fn run(&self, input: WriteInput) -> Result<String, ToolError> {
        let _ = (&self.sandbox, input);
        todo!()
    }
}

#[cfg(test)]
mod tests;
