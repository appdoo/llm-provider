//! The Read tool: a window of a file's lines, numbered from 1.
//!
//! Every read streams the file line by line, so memory stays bounded to the
//! window. A file over 10 MiB must be read with `offset` or
//! `limit`. Reaching a large `offset` still scans the file from its start.

use std::path::Path;

use async_trait::async_trait;
use futures::{StreamExt, TryStreamExt};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_stream::wrappers::LinesStream;

use super::{FileTool, Sandbox, ToolError};

/// Files larger than this must be read with `offset` or `limit`.
const READ_SIZE_CAP: u64 = 10 * 1024 * 1024;

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
        let path = self.sandbox.resolve(&input.path)?;
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(ToolError::io(&path))?;
        if !meta.is_file() {
            return Err(ToolError::NotAFile(input.path));
        }

        let windowed = input.offset.is_some() || input.limit.is_some();
        if meta.len() > READ_SIZE_CAP && !windowed {
            return Err(ToolError::InputTooLarge(
                "file too large — use offset/limit".into(),
            ));
        }

        let offset = input.offset.unwrap_or(0);
        let limit = input.limit.unwrap_or(DEFAULT_LIMIT);
        read_window(&path, offset, limit).await
    }
}

/// Lines `offset..offset + limit` of the file at `path`, each as
/// `<number>\t<line>`, joined by newlines.
async fn read_window(path: &Path, offset: usize, limit: usize) -> Result<String, ToolError> {
    let file = tokio::fs::File::open(path)
        .await
        .map_err(ToolError::io(path))?;
    let lines = LinesStream::new(BufReader::new(file).lines());

    let numbered: Vec<String> = lines
        .skip(offset)
        .take(limit)
        .enumerate()
        .map(|(index, line)| line.map(|line| format!("{}\t{line}", offset + index + 1)))
        .try_collect()
        .await
        .map_err(ToolError::io(path))?;
    Ok(numbered.join("\n"))
}

#[cfg(test)]
mod tests;
