//! The Glob tool: the files that match a glob pattern, newest first.
//!
//! A relative pattern is joined to the `path` base, resolved through the
//! sandbox; a match whose target lies outside the sandbox root is dropped.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{Sandbox, ToolError};

/// The input of the Glob tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GlobInput {
    /// Glob pattern to match files.
    pattern: String,
    /// Base directory for relative patterns; the sandbox root when absent.
    path: Option<String>,
}

/// Runs the Glob tool on `input`, inside `sandbox`.
pub(super) fn run(sandbox: &Sandbox, input: &GlobInput) -> Result<String, ToolError> {
    let base = sandbox.resolve(input.path.as_deref().unwrap_or("."))?;
    let pattern = base.join(&input.pattern);

    let mut paths: Vec<PathBuf> = glob::glob(&pattern.to_string_lossy())?
        .filter_map(Result::ok)
        .filter(|path| contains(sandbox, path))
        .collect();
    paths.sort_by_key(|path| Reverse(modified(path)));

    let lines: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    Ok(lines.join("\n"))
}

/// Whether `path`, symlinks resolved, lies inside the root of `sandbox`.
fn contains(sandbox: &Sandbox, path: &Path) -> bool {
    std::fs::canonicalize(path).is_ok_and(|real| real.starts_with(sandbox.root()))
}

/// When `path` was last modified, if the file system says.
fn modified(path: &Path) -> Option<SystemTime> {
    path.metadata().and_then(|meta| meta.modified()).ok()
}

#[cfg(test)]
mod tests;
