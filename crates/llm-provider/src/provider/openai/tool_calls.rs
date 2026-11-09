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
        let slot = self.slot_for(fragment.index, fragment.id.as_deref());
        self.last = Some(slot);

        let call = &mut self.slots[slot];
        if let (None, Some(id)) = (&call.id, fragment.id) {
            self.by_id.insert(id.clone(), slot);
            call.id = Some(id);
        }

        let function = fragment.function.unwrap_or_default();
        if call.name.is_none() {
            call.name = function.name;
        }
        call.arguments
            .push_str(&function.arguments.unwrap_or_default());
        if call.arguments.len() > MAX_TOOL_INPUT_BYTES {
            return Err(Error::TooLarge {
                what: Oversize::ToolInput,
                max: MAX_TOOL_INPUT_BYTES,
            });
        }

        Ok(())
    }

    /// The fragment's slot: by `index`, else by a known `id`, else the last
    /// slot when the fragment has neither; a new slot otherwise.
    fn slot_for(&mut self, index: Option<u64>, id: Option<&str>) -> usize {
        let existing = match (index, id) {
            (Some(index), _) => self.by_index.get(&index).copied(),
            (None, Some(id)) => self.by_id.get(id).copied(),
            (None, None) => self.last,
        };
        if let Some(slot) = existing {
            return slot;
        }

        self.slots.push(PartialCall::default());
        let slot = self.slots.len() - 1;
        if let Some(index) = index {
            self.by_index.insert(index, slot);
        }

        slot
    }

    /// Every call, once: in index order when every call came with an index,
    /// else in creation order. Empty arguments read as `{}`, invalid JSON is
    /// [`Error::Json`]. Later calls return nothing.
    pub(super) fn flush(&mut self) -> Result<Vec<ToolUseCall>, Error> {
        if std::mem::replace(&mut self.flushed, true) {
            return Ok(Vec::new());
        }

        let order = self.order();
        let mut slots: Vec<Option<PartialCall>> = std::mem::take(&mut self.slots)
            .into_iter()
            .map(Some)
            .collect();

        order
            .into_iter()
            .filter_map(|slot| slots[slot].take())
            .map(PartialCall::finish)
            .collect()
    }

    /// The slots in index order when every slot has an index, else in
    /// creation order.
    fn order(&self) -> Vec<usize> {
        if self.by_index.len() != self.slots.len() {
            return (0..self.slots.len()).collect();
        }

        let mut indexed: Vec<(u64, usize)> = self.by_index.iter().map(|(&i, &s)| (i, s)).collect();
        indexed.sort_unstable();
        indexed.into_iter().map(|(_, slot)| slot).collect()
    }
}

impl PartialCall {
    /// The finished call; empty arguments read as `{}`.
    fn finish(self) -> Result<ToolUseCall, Error> {
        let input = if self.arguments.is_empty() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(&self.arguments)?
        };

        Ok(ToolUseCall {
            id: self.id.unwrap_or_default(),
            name: self.name.unwrap_or_default(),
            input,
        })
    }
}
