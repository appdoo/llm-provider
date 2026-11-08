//! [`Files`]: the five file tools as one `#[tools]` block over one
//! [`Sandbox`].

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
        read::run(&self.sandbox, input).await
    }

    /// Create or overwrite a file. Creates parent directories automatically.
    #[tool]
    async fn write(&self, input: WriteInput) -> Result<String, ToolError> {
        write::run(&self.sandbox, input).await
    }

    /// Replace an exact string in a file. The old_string must appear exactly once.
    // The doc line is the tool description the model reads.
    #[allow(clippy::doc_markdown)]
    #[tool]
    async fn edit(&self, input: EditInput) -> Result<String, ToolError> {
        edit::run(&self.sandbox, input).await
    }

    /// Search file contents with regex
    #[tool]
    async fn grep(&self, input: GrepInput) -> Result<String, ToolError> {
        grep::run(&self.sandbox, input).await
    }

    /// Find files matching a glob pattern
    // A #[tool] method is async; matching paths awaits nothing.
    #[allow(clippy::unused_async)]
    #[tool]
    async fn glob(&self, input: GlobInput) -> Result<String, ToolError> {
        glob::run(&self.sandbox, &input)
    }
}
