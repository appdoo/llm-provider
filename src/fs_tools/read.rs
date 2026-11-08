//! The Read tool: a window of a file's lines, numbered from 1.
//!
//! Every read streams the file line by line, so memory stays bounded to the
//! window. A file over [`READ_SIZE_CAP`] must be read with `offset` or
//! `limit`. Reaching a large `offset` still scans the file from its start.

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{FileTool, Sandbox, ToolError};

/// Files larger than this must be read with `offset` or `limit`.
pub(crate) const READ_SIZE_CAP: u64 = 10 * 1024 * 1024;

/// How many lines a read returns when the input sets no `limit`.
const DEFAULT_LIMIT: usize = 2000;

/// The Read tool: returns lines of a file inside its sandbox as
/// `<number>\t<line>`.
#[derive(Debug, Clone)]
pub struct ReadTool {
    sandbox: Sandbox,
}

/// The input of the Read tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ReadInput {
    /// File path to read.
    path: String,
    /// Start line (0-indexed).
    offset: Option<usize>,
    /// Max lines to read.
    limit: Option<usize>,
}

impl ReadTool {
    /// A Read tool confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::fs_tools::{ReadTool, Sandbox};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let read = ReadTool::new(Sandbox::new("docs")?);
    /// # let _ = read;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[async_trait]
impl FileTool for ReadTool {
    type Input = ReadInput;
    const NAME: &'static str = "Read";
    const DESCRIPTION: &'static str = "Read file contents with optional offset and limit";

    async fn run(&self, input: ReadInput) -> Result<String, ToolError> {
        let _ = (&self.sandbox, input);
        todo!()
    }
}

#[cfg(test)]
mod tests;
