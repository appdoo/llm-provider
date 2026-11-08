//! The Grep tool: the lines of the files under a path that match a regular
//! expression, with optional context lines.
//!
//! The walk skips hidden entries and does not follow symlinks.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{FileTool, Sandbox, ToolError};

/// The most context lines a search shows around a match.
const MAX_CONTEXT: usize = 10_000;

/// The Grep tool: searches the files inside its sandbox.
#[derive(Debug, Clone)]
pub struct GrepTool {
    sandbox: Sandbox,
}

/// The input of the Grep tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GrepInput {
    /// Regex pattern to search for.
    pattern: String,
    /// File or directory to search; the sandbox root when absent.
    path: Option<String>,
    /// Number of context lines around matches.
    context: Option<usize>,
}

impl GrepTool {
    /// A Grep tool confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::fs_tools::{GrepTool, Sandbox};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let grep = GrepTool::new(Sandbox::new("src")?);
    /// # let _ = grep;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl FileTool for GrepTool {
    type Input = GrepInput;
    const NAME: &'static str = "Grep";
    const DESCRIPTION: &'static str = "Search file contents with regex";

    async fn run(&self, input: GrepInput) -> Result<String, ToolError> {
        let _ = (&self.sandbox, input, MAX_CONTEXT);
        todo!()
    }
}

#[cfg(test)]
mod tests;
