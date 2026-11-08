//! Sandboxed file tools (feature `fs-tools`): Read, Write, Edit, Glob and
//! Grep, each confined to one [`Sandbox`].
//!
//! ```no_run
//! use llm_provider::agent::Tools;
//! use llm_provider::fs_tools::{EditTool, ReadTool, Sandbox};
//!
//! # fn build() -> Result<(), Box<dyn std::error::Error>> {
//! let sandbox = Sandbox::new("docs")?;
//! let tools = Tools::new()
//!     .with(ReadTool::new(sandbox.clone()))
//!     .with(EditTool::new(sandbox));
//! # let _ = tools;
//! # Ok(())
//! # }
//! ```

pub mod edit;
mod error;
pub mod glob;
pub mod grep;
pub mod read;
mod sandbox;
pub mod write;

pub use edit::EditTool;
pub use error::ToolError;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;
pub use sandbox::Sandbox;
pub use write::WriteTool;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::agent::{Tool, ToolOutput};
use crate::provider::ToolDefinition;

/// A file tool: its typed input and a body that fails with a [`ToolError`].
/// Every file tool is a [`Tool`] through it.
#[async_trait]
trait FileTool: Send + Sync {
    /// The input the model sends, parsed by `serde`; its schema is derived.
    type Input: DeserializeOwned + JsonSchema + Send;

    /// The name the model calls the tool by.
    const NAME: &'static str;

    /// What the tool does, as the model reads it.
    const DESCRIPTION: &'static str;

    /// Runs the tool on its parsed input.
    async fn run(&self, input: Self::Input) -> Result<String, ToolError>;
}

#[async_trait]
impl<T: FileTool> Tool for T {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: T::NAME.into(),
            description: T::DESCRIPTION.into(),
            input_schema: input_schema::<T::Input>(),
        }
    }

    async fn call(&self, input: serde_json::Value) -> ToolOutput {
        let result = match serde_json::from_value(input) {
            Ok(input) => self.run(input).await,
            Err(error) => Err(ToolError::InvalidInput(error)),
        };
        ToolOutput::from(result)
    }
}

/// The JSON schema of `I`, without the `$schema` and `title` keys a tool
/// definition does not carry.
fn input_schema<I: JsonSchema>() -> serde_json::Value {
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.meta_schema = None)
        .into_generator()
        .into_root_schema_for::<I>();
    schema.remove("title");
    schema.to_value()
}

#[cfg(test)]
mod tests;
