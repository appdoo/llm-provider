//! Test-only scripts for agent tests: the replies a `ScriptedProvider`
//! plays, and [`scripted`], the model that plays them.

use serde_json::json;

use crate::provider::test_support::ScriptedProvider;
use crate::provider::{Llm, StreamEvent, ToolUseCall};

/// A reply that streams `text` and ends.
pub(crate) fn text(text: &str) -> Vec<StreamEvent> {
    vec![
        StreamEvent::ContentDelta(text.into()),
        StreamEvent::MessageStop,
    ]
}

/// A tool call `id` to the tool `name` with an empty input.
pub(crate) fn tool_use(id: &str, name: &str) -> StreamEvent {
    tool_use_with(id, name, json!({}))
}

/// A tool call `id` to the tool `name` with `input`.
pub(crate) fn tool_use_with(id: &str, name: &str, input: serde_json::Value) -> StreamEvent {
    StreamEvent::ToolUse(ToolUseCall {
        id: id.into(),
        name: name.into(),
        input,
    })
}

/// A reply that asks for `calls` and ends.
pub(crate) fn calls(calls: Vec<StreamEvent>) -> Vec<StreamEvent> {
    calls
        .into_iter()
        .chain([StreamEvent::MessageStop])
        .collect()
}

/// `provider` as the model an agent test runs on.
pub(crate) fn scripted(provider: ScriptedProvider) -> Llm {
    Llm::from_provider(provider)
}
