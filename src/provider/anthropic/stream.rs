//! Decoding the Messages API event stream into [`StreamEvent`]s.

use serde::Deserialize;

use crate::provider::anthropic::response::{WireError, WireStopReason, WireUsage};
use crate::provider::error::Error;
use crate::provider::event::{StopReason, StreamEvent};
use crate::provider::EventStream;
use crate::transport::ByteStream;

/// The largest `data:` payload of one event, in bytes.
const MAX_EVENT_BYTES: usize = 1024 * 1024;
/// The largest streamed input of one tool call, in bytes.
const MAX_TOOL_INPUT_BYTES: usize = 1024 * 1024;

/// Decodes a response body into events. A body without bytes is
/// [`Error::EmptyResponse`]; the first error ends the stream. A stop reason
/// is emitted as [`StreamEvent::Stop`] right before
/// [`StreamEvent::MessageStop`].
pub(crate) fn decode(bytes: ByteStream) -> EventStream {
    todo!()
}

/// Turns the lines of one event stream into events. An empty line ends an
/// event; `data:` lines build its payload.
#[derive(Debug, Default)]
pub(crate) struct EventParser {
    data: String,
    tool: Option<PendingTool>,
    stop: Option<StopReason>,
}

/// A tool call whose input is still streaming.
#[derive(Debug)]
struct PendingTool {
    id: String,
    name: String,
    input: String,
}

impl EventParser {
    /// The events `line` completes, in order: none until an event ends.
    /// A payload over 1 MiB is [`Error::TooLarge`], an `error` event is
    /// [`Error::Api`].
    pub(crate) fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error> {
        todo!()
    }

    /// The events one parsed wire event stands for.
    fn apply(&mut self, event: WireEvent) -> Result<Vec<StreamEvent>, Error> {
        todo!()
    }

    /// Appends a fragment of the pending tool's input; over 1 MiB is
    /// [`Error::TooLarge`].
    fn push_tool_input(&mut self, fragment: &str) -> Result<(), Error> {
        todo!()
    }

    /// The pending tool call as [`StreamEvent::ToolUse`]; empty input reads
    /// as `{}`, invalid JSON is [`Error::ToolInput`].
    fn finish_tool(&mut self) -> Result<Vec<StreamEvent>, Error> {
        todo!()
    }
}

/// One server-sent event's payload, keyed by its `type`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireEvent {
    MessageStart {
        message: StartMessage,
    },
    ContentBlockStart {
        content_block: StartBlock,
    },
    ContentBlockDelta {
        delta: Delta,
    },
    ContentBlockStop {},
    MessageDelta {
        delta: MessageDeltaBody,
        #[serde(default)]
        usage: Option<DeltaUsage>,
    },
    MessageStop {},
    Error {
        error: WireError,
    },
    #[serde(other)]
    Other,
}

/// The message of `message_start`; its usage carries the input counters.
#[derive(Debug, Deserialize)]
struct StartMessage {
    #[serde(default)]
    usage: Option<WireUsage>,
}

/// The block opened by `content_block_start`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StartBlock {
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Other,
}

/// The change carried by `content_block_delta`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
    #[serde(other)]
    Other,
}

/// The `delta` of `message_delta`.
#[derive(Debug, Deserialize)]
struct MessageDeltaBody {
    #[serde(default)]
    stop_reason: Option<WireStopReason>,
}

/// The `usage` of `message_delta`: the cumulative output count.
#[derive(Debug, Deserialize)]
struct DeltaUsage {
    #[serde(default)]
    output_tokens: u32,
}
