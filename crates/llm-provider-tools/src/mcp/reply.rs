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
    /// A kind of block this crate does not know yet.
    Unsupported,
}

impl Reply {
    /// The tool output: text blocks as they are, an image or audio block as
    /// `[image <mime>]` / `[audio <mime>]`, a resource as `[resource <uri>]`
    /// and its text, joined with blank lines; with no text block, the
    /// structured result as JSON.
    pub(crate) fn into_output(self) -> ToolOutput {
        let has_text = self
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Text(_)));
        let mut parts: Vec<String> = self.blocks.into_iter().map(Block::render).collect();

        if let (false, Some(structured)) = (has_text, self.structured) {
            parts.push(structured.to_string());
        }
        ToolOutput {
            content: parts.join("\n\n"),
            is_error: self.is_error,
        }
    }
}

impl Block {
    /// The block as the model reads it.
    fn render(self) -> String {
        match self {
            Block::Text(text) => text,
            Block::Image { mime_type } => format!("[image {mime_type}]"),
            Block::Audio { mime_type } => format!("[audio {mime_type}]"),
            Block::Resource { uri, text: None } => format!("[resource {uri}]"),
            Block::Resource {
                uri,
                text: Some(text),
            } => format!("[resource {uri}]\n{text}"),
            Block::Unsupported => "[unsupported content]".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests;
