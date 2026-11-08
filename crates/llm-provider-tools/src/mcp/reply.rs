//! [`Reply`]: a server's answer to a tool call, and the text the model
//! reads from it.

use llm_provider::agent::ToolOutput;

/// A server's answer to one tool call, free of the MCP client's types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reply {
    /// The content blocks, in order.
    pub(crate) blocks: Vec<Block>,
    /// The structured result, if the server sent one.
    pub(crate) structured: Option<serde_json::Value>,
    /// The server's error flag.
    pub(crate) is_error: bool,
}

/// One content block of a [`Reply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// Text the model reads as it is.
    Text(String),
    /// An image, shown to the model by its media type.
    Image { mime_type: String },
    /// Audio, shown to the model by its media type.
    Audio { mime_type: String },
    /// An embedded or linked resource: its URI, and its text if it carries
    /// text.
    Resource { uri: String, text: Option<String> },
}

impl Reply {
    /// The tool output: text blocks as they are, an image or audio block as
    /// `[image <mime>]` / `[audio <mime>]`, a resource as `[resource <uri>]`
    /// and its text, joined with blank lines; with no text block, the
    /// structured result as JSON.
    pub(crate) fn into_output(self) -> ToolOutput {
        todo!()
    }
}

#[cfg(test)]
mod tests;
