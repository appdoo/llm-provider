//! The Glob tool: the files that match a glob pattern, newest first.
//!
//! A relative pattern is joined to the `path` base, resolved through the
//! sandbox; a match whose target lies outside the sandbox root is dropped.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
        let base = self.sandbox.resolve(input.path.as_deref().unwrap_or("."))?;
        let pattern = base.join(&input.pattern);

        let mut paths: Vec<PathBuf> = glob::glob(&pattern.to_string_lossy())?
            .filter_map(Result::ok)
            .filter(|path| self.contains(path))
            .collect();
        paths.sort_by_key(|path| Reverse(modified(path)));

        let lines: Vec<String> = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        Ok(lines.join("\n"))
    }
}

impl GlobTool {
    /// Whether `path`, symlinks resolved, lies inside the sandbox root.
    fn contains(&self, path: &Path) -> bool {
        std::fs::canonicalize(path).is_ok_and(|real| real.starts_with(self.sandbox.root()))
    }
}

/// When `path` was last modified, if the file system says.
fn modified(path: &Path) -> Option<SystemTime> {
    path.metadata().and_then(|meta| meta.modified()).ok()
}

#[cfg(test)]
mod tests;
