//! [`ToolCalls`]: tool calls assembled from their streamed fragments. A
//! fragment finds its call by `index`, else by `id`, else it continues the
//! last one.

use std::collections::HashMap;

use serde::Deserialize;

use crate::provider::completion::ToolUseCall;
use crate::provider::error::{Error, Oversize};

/// The largest streamed input of one tool call, in bytes.
pub(super) const MAX_TOOL_INPUT_BYTES: usize = 1024 * 1024;

/// One streamed piece of a tool call.
#[derive(Debug, Deserialize)]
pub(super) struct ToolCallFragment {
    #[serde(default)]
    index: Option<u64>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionFragment>,
}

/// The function part of a [`ToolCallFragment`].
#[derive(Debug, Default, Deserialize)]
struct FunctionFragment {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// The tool calls of one answer while they stream.
#[derive(Debug, Default)]
pub(super) struct ToolCalls {
    /// The calls in creation order.
    slots: Vec<PartialCall>,
    /// `index` → slot, for fragments that carry an index.
    by_index: HashMap<u64, usize>,
    /// `id` → slot, for continuation fragments without an index.
    by_id: HashMap<String, usize>,
    /// The slot the previous fragment used.
    last: Option<usize>,
    /// Whether the calls were handed out.
    flushed: bool,
}

/// One tool call assembled so far.
#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCalls {
    /// Adds `fragment` to its call; arguments over 1 MiB are
    /// [`Error::TooLarge`].
    pub(super) fn push(&mut self, fragment: ToolCallFragment) -> Result<(), Error> {
        todo!()
    }

    /// Every call, once: in index order when every call came with an index,
    /// else in creation order. Empty arguments read as `{}`, invalid JSON is
    /// [`Error::Json`]. Later calls return nothing.
    pub(super) fn flush(&mut self) -> Result<Vec<ToolUseCall>, Error> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
