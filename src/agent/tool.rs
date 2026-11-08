//! The tools a model can call: one [`Tool`], a block of them
//! ([`IntoTools`]), the [`Toolbox`] a runtime dispatches through,
//! [`Tools`], a toolbox of boxed tools, and the [`Transcript`] of the
//! calling conversation every call can read.

use std::fmt;

use async_trait::async_trait;

use crate::provider::{Message, ToolDefinition};

/// The calling agent's conversation at the moment of a tool call,
/// read-only: its system prompt, its tools and its messages.
///
/// [`render`](Self::render) is the one place a conversation becomes text
/// for another model; that text belongs in a user message, never in a
/// system prompt.
#[derive(Debug, Clone, Copy, Default)]
pub struct Transcript<'a> {
    /// The calling agent's system prompt, if it has one.
    pub system: Option<&'a str>,
    /// The tools the calling agent offers its model.
    pub tools: &'a [ToolDefinition],
    /// The conversation so far, oldest first.
    pub messages: &'a [Message],
}

impl Transcript<'_> {
    /// The conversation as text a model reads as data: the system prompt,
    /// the tool names, then each message, tool call and tool result in
    /// order, under plain labels.
    #[must_use]
    pub fn render(&self) -> String {
        todo!()
    }
}

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

    /// Runs the tool on the model's input; `transcript` is the calling
    /// conversation, which the tool may read or ignore.
    async fn call(&self, input: serde_json::Value, transcript: &Transcript<'_>) -> ToolOutput;
}

/// A block of tools that share one value, as `#[tools]` on an impl block
/// writes it; [`Tools::with_all`] adds them all.
pub trait IntoTools {
    /// The block's tools, in source order, each holding a share of the value.
    fn into_tools(self) -> Vec<Box<dyn Tool>>;
}

/// The tools a [`ConversationRuntime`](crate::agent::ConversationRuntime)
/// offers the model; it finds a call by name.
#[async_trait]
pub trait Toolbox: Send + Sync {
    /// The definitions of every tool in the box, sent with each request.
    fn definitions(&self) -> Vec<ToolDefinition>;

    /// Runs the tool called `name` on `input`, handing it the calling
    /// conversation.
    async fn call(
        &self,
        name: &str,
        input: serde_json::Value,
        transcript: &Transcript<'_>,
    ) -> ToolOutput;
}

/// A [`Toolbox`] of boxed tools, such as the `fs_tools`.
///
/// ```no_run
/// # #[cfg(feature = "fs-tools")]
/// # fn build() -> Result<(), Box<dyn std::error::Error>> {
/// use llm_provider::agent::Tools;
/// use llm_provider::fs_tools::{Files, Sandbox};
///
/// let tools = Tools::new()
///     .with_all(Files::new(Sandbox::new("docs")?))
///     .only(["read", "edit"]);
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
    ///
    /// # Panics
    /// When the set already holds a tool of the same name; the message
    /// names it.
    #[must_use]
    pub fn with(mut self, tool: impl Tool + 'static) -> Self {
        self.push(Box::new(tool));
        self
    }

    /// The set with every tool of `block` added, in the block's order.
    ///
    /// # Panics
    /// When a tool of the block has the name of a tool already in the set;
    /// the message names it.
    #[must_use]
    pub fn with_all(mut self, block: impl IntoTools) -> Self {
        for tool in block.into_tools() {
            self.push(tool);
        }
        self
    }

    /// Adds `tool`, panicking when its name is taken.
    fn push(&mut self, tool: Box<dyn Tool>) {
        let name = tool.definition().name;
        let taken = self.0.iter().any(|kept| kept.definition().name == name);
        assert!(!taken, "a tool named {name:?} is already in the set");
        self.0.push(tool);
    }

    /// The set keeping only the tools named in `names`, in the order they
    /// were added.
    #[must_use]
    pub fn only<N: AsRef<str>>(mut self, names: impl IntoIterator<Item = N>) -> Self {
        let names: Vec<N> = names.into_iter().collect();
        self.0.retain(|tool| {
            let name = tool.definition().name;
            names.iter().any(|kept| kept.as_ref() == name)
        });
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

    async fn call(
        &self,
        name: &str,
        input: serde_json::Value,
        transcript: &Transcript<'_>,
    ) -> ToolOutput {
        let Some(tool) = self.0.iter().find(|tool| tool.definition().name == name) else {
            return ToolOutput {
                content: format!("unknown tool {name}"),
                is_error: true,
            };
        };
        tool.call(input, transcript).await
    }
}

#[cfg(test)]
mod tests;
