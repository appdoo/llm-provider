//! The Glob tool: the files that match a glob pattern, newest first.
//!
//! A relative pattern is joined to the `path` base, resolved through the
//! sandbox; a match whose target lies outside the sandbox root is dropped.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{FileTool, Sandbox, ToolError};

/// The Glob tool: finds files inside its sandbox.
#[derive(Debug, Clone)]
pub struct GlobTool {
    sandbox: Sandbox,
}

/// The input of the Glob tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GlobInput {
    /// Glob pattern to match files.
    pattern: String,
    /// Base directory for relative patterns; the sandbox root when absent.
    path: Option<String>,
}

impl GlobTool {
    /// A Glob tool confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::fs_tools::{GlobTool, Sandbox};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let glob = GlobTool::new(Sandbox::new("src")?);
    /// # let _ = glob;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl FileTool for GlobTool {
    type Input = GlobInput;
    const NAME: &'static str = "Glob";
    const DESCRIPTION: &'static str = "Find files matching a glob pattern";

    async fn run(&self, input: GlobInput) -> Result<String, ToolError> {
        let _ = (&self.sandbox, input);
        todo!()
    }
}

#[cfg(test)]
mod tests;
