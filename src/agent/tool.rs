//! The tools a model can call: one [`Tool`], the [`Toolbox`] a runtime
//! dispatches through, and [`Tools`], a toolbox of boxed tools.

use std::fmt;

use async_trait::async_trait;

use crate::provider::ToolDefinition;

/// What a tool hands back to the model: its text, and whether it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// The text the model reads as the tool's result.
    pub content: String,
    /// Whether the call failed; the model sees the result as an error.
    pub is_error: bool,
}

/// One tool the model can call.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The name, description and input schema the model sees.
    fn definition(&self) -> ToolDefinition;

    /// Runs the tool on the model's input.
    async fn call(&self, input: serde_json::Value) -> ToolOutput;
}

/// The tools a [`ConversationRuntime`](crate::agent::ConversationRuntime)
/// offers the model; it finds a call by name.
#[async_trait]
pub trait Toolbox: Send + Sync {
    /// The definitions of every tool in the box, sent with each request.
    fn definitions(&self) -> Vec<ToolDefinition>;

    /// Runs the tool called `name` on `input`.
    async fn call(&self, name: &str, input: serde_json::Value) -> ToolOutput;
}

/// A [`Toolbox`] of boxed tools, such as the `fs_tools`.
///
/// ```no_run
/// # #[cfg(feature = "fs-tools")]
/// # fn build() -> Result<(), Box<dyn std::error::Error>> {
/// use llm_provider::agent::Tools;
/// use llm_provider::fs_tools::{ReadTool, Sandbox};
///
/// let tools = Tools::new().with(ReadTool::new(Sandbox::new("docs")?));
/// # let _ = tools;
/// # Ok(())
/// # }
/// ```
#[derive(Default)]
pub struct Tools(Vec<Box<dyn Tool>>);

impl Tools {
    /// An empty set of tools.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The set with `tool` added.
    #[must_use]
    pub fn with(mut self, tool: impl Tool + 'static) -> Self {
        self.0.push(Box::new(tool));
        self
    }
}

// Hand-written: `dyn Tool` has no `Debug`; the tool names stand for it.
impl fmt::Debug for Tools {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = self.0.iter().map(|tool| tool.definition().name);
        f.debug_tuple("Tools")
            .field(&names.collect::<Vec<_>>())
            .finish()
    }
}

#[async_trait]
impl Toolbox for Tools {
    fn definitions(&self) -> Vec<ToolDefinition> {
        self.0.iter().map(|tool| tool.definition()).collect()
    }

    async fn call(&self, name: &str, input: serde_json::Value) -> ToolOutput {
        todo!()
    }
}

// Input helpers of the file tools; they leave with wave C of Spec 36.

#[cfg(feature = "fs-tools")]
/// The string field `field` of `input`, or the uniform error
/// `"field 'X' is required and must be a string"`.
pub(crate) fn require_string<'a>(
    input: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, String> {
    input
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("field '{field}' is required and must be a string"))
}

#[cfg(feature = "fs-tools")]
/// The non-empty string field `field` of `input`; an empty one is
/// `"field 'X' is required and must not be empty"`.
pub(crate) fn require_string_nonempty<'a>(
    input: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, String> {
    let s = require_string(input, field)?;
    if s.is_empty() {
        return Err(format!("field '{field}' is required and must not be empty"));
    }
    Ok(s)
}

#[cfg(feature = "fs-tools")]
/// The `u64` field `field` of `input`, `default` when absent or not a
/// number, clamped into `[min, max]`.
pub(crate) fn optional_u64_clamped(
    input: &serde_json::Value,
    field: &str,
    default: u64,
    min: u64,
    max: u64,
) -> u64 {
    input
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(default)
        .clamp(min, max)
}

#[cfg(test)]
mod tests;
