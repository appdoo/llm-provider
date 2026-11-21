//! The Read tool: a window of a file's lines, numbered from 1.
//!
//! Every read streams the file line by line, so memory stays bounded to the
//! window. A file over 10 MiB must be read with `offset` or
//! `limit`. Reaching a large `offset` still scans the file from its start.

use std::io::{BufRead, BufReader};
use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Sandbox, ToolError};

/// Files larger than this must be read with `offset` or `limit`.
const READ_SIZE_CAP: u64 = 10 * 1024 * 1024;

/// How many lines a read returns when the input sets no `limit`.
const DEFAULT_LIMIT: usize = 2000;

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

/// Runs the Read tool on `input`, inside `sandbox`.
pub(super) fn run(sandbox: &Sandbox, input: ReadInput) -> Result<String, ToolError> {
    let path = sandbox.resolve(&input.path)?;
    let meta = std::fs::metadata(&path).map_err(ToolError::io(&path))?;
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
    read_window(&path, offset, limit)
}

/// Lines `offset..offset + limit` of the file at `path`, each as
/// `<number>\t<line>`, joined by newlines.
fn read_window(path: &Path, offset: usize, limit: usize) -> Result<String, ToolError> {
    let file = std::fs::File::open(path).map_err(ToolError::io(path))?;
    let lines = BufReader::new(file).lines();

    let numbered: Vec<String> = lines
        .skip(offset)
        .take(limit)
        .enumerate()
        .map(|(index, line)| line.map(|line| format!("{}\t{line}", offset + index + 1)))
        .collect::<Result<_, std::io::Error>>()
        .map_err(ToolError::io(path))?;
    Ok(numbered.join("\n"))
}

#[cfg(test)]
mod tests;
