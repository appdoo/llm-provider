//! [`Files`]: the five file tools as one `#[tools]` block over one
//! [`Sandbox`].

use super::blocking::blocking;
use super::edit::{self, EditInput};
use super::glob::{self, GlobInput};
use super::grep::{self, GrepInput};
use super::read::{self, ReadInput};
use super::write::{self, WriteInput};
use super::{Sandbox, ToolError};
use crate::agent::tools;

/// The file tools `read`, `write`, `edit`, `grep` and `glob`, each confined
/// to one [`Sandbox`]; [`Tools::with_all`](crate::agent::Tools::with_all)
/// adds them.
///
/// `read` returns a window of a file's lines, numbered from 1; it refuses
/// a file over 10 MiB unless the call sets `offset` or `limit`.
#[derive(Debug, Clone)]
pub struct Files {
    sandbox: Sandbox,
}

impl Files {
    /// The file tools confined to `sandbox`.
    ///
    /// ```no_run
    /// use llm_provider::agent::Tools;
    /// use llm_provider::fs_tools::{Files, Sandbox};
    ///
    /// # fn build() -> Result<(), Box<dyn std::error::Error>> {
    /// let tools = Tools::new().with_all(Files::new(Sandbox::new("docs")?));
    /// # let _ = tools;
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn new(sandbox: Sandbox) -> Self {
        Self { sandbox }
    }
}

#[tools]
impl Files {
    /// Read file contents with optional offset and limit
    #[tool]
    async fn read(&self, input: ReadInput) -> Result<String, ToolError> {
        let sandbox = self.sandbox.clone();
        blocking(move || read::run(&sandbox, input)).await
    }

    /// Create or overwrite a file. Creates parent directories automatically.
    #[tool]
    async fn write(&self, input: WriteInput) -> Result<String, ToolError> {
        let sandbox = self.sandbox.clone();
        blocking(move || write::run(&sandbox, &input)).await
    }

    /// Replace an exact string in a file. The old_string must appear exactly once.
    // The doc line is the tool description the model reads.
    #[allow(clippy::doc_markdown)]
    #[tool]
    async fn edit(&self, input: EditInput) -> Result<String, ToolError> {
        let sandbox = self.sandbox.clone();
        blocking(move || edit::run(&sandbox, input)).await
    }

    /// Search file contents with regex
    #[tool]
    async fn grep(&self, input: GrepInput) -> Result<String, ToolError> {
        let sandbox = self.sandbox.clone();
        blocking(move || grep::run(&sandbox, &input)).await
    }

    /// Find files matching a glob pattern
    #[tool]
    async fn glob(&self, input: GlobInput) -> Result<String, ToolError> {
        let sandbox = self.sandbox.clone();
        blocking(move || glob::run(&sandbox, &input)).await
    }
}
