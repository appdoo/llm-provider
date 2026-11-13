//! Decoding the Messages API event stream into [`StreamEvent`]s.

use serde::Deserialize;
use tokio_stream::StreamExt;

use crate::provider::anthropic::response::{WireError, WireStopReason, WireUsage};
use crate::provider::error::{Error, Oversize};
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent, ToolUseCall, Usage};
use crate::transport::{sse_data, ByteStream, LineAccumulator};

/// The largest `data:` payload of one event, in bytes.
const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// The largest streamed input of one tool call, in bytes.
const MAX_TOOL_INPUT_BYTES: usize = 1024 * 1024;

/// Decodes a response body into events. A body without bytes is
/// [`Error::EmptyResponse`]; the first error ends the stream. A stop reason
/// is emitted as [`StreamEvent::Stop`] right before
/// [`StreamEvent::MessageStop`]. A body that ends before `message_stop` is
/// [`Error::UnexpectedEnd`].
pub(crate) fn decode(mut bytes: ByteStream) -> EventStream {
    Box::pin(async_stream::try_stream! {
        let mut lines = LineAccumulator::new();
        let mut parser = EventParser::default();
        let mut received = false;

        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            received |= !chunk.is_empty();
            lines.extend(&chunk);
            while let Some(line) = lines.next_line()? {
                for event in parser.feed_line(&line)? {
                    yield event;
                }
            }
        }

        if !received {
            Err(Error::EmptyResponse)?;
        }
        lines.finish()?;
        if !parser.stopped {
            Err(Error::UnexpectedEnd)?;
        }
    })
}

/// Turns the lines of one event stream into events. An empty line ends an
/// event; `data:` lines build its payload, whose `type` names the event.
#[derive(Debug, Default)]
struct EventParser {
    data: String,
    tool: Option<PendingTool>,
    stop: Option<StopReason>,
    /// Whether `message_stop` has been seen.
    stopped: bool,
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
    fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error> {
        if line.is_empty() {
            return self.end_event();
        }

        if let Some(payload) = sse_data(line) {
            self.push_data(payload)?;
        }

        Ok(Vec::new())
    }

    /// Appends one `data:` payload; over 1 MiB is [`Error::TooLarge`].
    fn push_data(&mut self, payload: &str) -> Result<(), Error> {
        self.data.push_str(payload);
        if self.data.len() > MAX_EVENT_BYTES {
            return Err(Error::TooLarge {
                what: Oversize::Event,
                max: MAX_EVENT_BYTES,
            });
        }

        Ok(())
    }

    /// Parses the buffered payload, if any, and applies it.
    fn end_event(&mut self) -> Result<Vec<StreamEvent>, Error> {
        if self.data.is_empty() {
            return Ok(Vec::new());
        }

        let data = std::mem::take(&mut self.data);
        let event: WireEvent = serde_json::from_str(&data)?;

        self.apply(event)
    }

    /// The events one parsed wire event stands for.
    fn apply(&mut self, event: WireEvent) -> Result<Vec<StreamEvent>, Error> {
        match event {
            WireEvent::MessageStart { message } => {
                Ok(message.usage.map(start_usage).into_iter().collect())
            }
            WireEvent::ContentBlockStart { content_block } => {
                self.start_block(content_block);
                Ok(Vec::new())
            }
            WireEvent::ContentBlockDelta { delta } => self.apply_delta(delta),
            WireEvent::ContentBlockStop {} => self.finish_tool(),
            WireEvent::MessageDelta { delta, usage } => {
                if let Some(reason) = delta.stop_reason {
                    self.stop = Some(reason.into());
                }

                Ok(usage.map(delta_usage).into_iter().collect())
            }
            WireEvent::MessageStop {} => Ok(self.message_stop()),
            WireEvent::Error { error } => Err(Error::Api(error.into())),
            WireEvent::Other => Ok(Vec::new()),
        }
    }

    /// Opens a pending tool call when `block` is a `tool_use`.
    fn start_block(&mut self, block: StartBlock) {
        if let StartBlock::ToolUse { id, name } = block {
            self.tool = Some(PendingTool {
                id,
                name,
                input: String::new(),
            });
        }
    }

    /// A text delta as [`StreamEvent::ContentDelta`]; a tool-input fragment
    /// is buffered.
    fn apply_delta(&mut self, delta: Delta) -> Result<Vec<StreamEvent>, Error> {
        match delta {
            Delta::Text { text } => Ok(vec![StreamEvent::ContentDelta(text)]),
            Delta::InputJson { partial_json } => {
                self.push_tool_input(&partial_json)?;
                Ok(Vec::new())
            }
            Delta::Other => Ok(Vec::new()),
        }
    }

    /// [`StreamEvent::Stop`] with the stored stop reason, if one came, then
    /// [`StreamEvent::MessageStop`].
    fn message_stop(&mut self) -> Vec<StreamEvent> {
        self.stopped = true;
        let stop = self.stop.take().map(StreamEvent::Stop);
        stop.into_iter().chain([StreamEvent::MessageStop]).collect()
    }

    /// Appends a fragment of the pending tool's input; over 1 MiB is
    /// [`Error::TooLarge`]. Without a pending tool the fragment is dropped.
    fn push_tool_input(&mut self, fragment: &str) -> Result<(), Error> {
        let Some(tool) = self.tool.as_mut() else {
            return Ok(());
        };

        tool.input.push_str(fragment);
        if tool.input.len() > MAX_TOOL_INPUT_BYTES {
            return Err(Error::TooLarge {
                what: Oversize::ToolInput,
                max: MAX_TOOL_INPUT_BYTES,
            });
        }

        Ok(())
    }

    /// The pending tool call as [`StreamEvent::ToolUse`]; empty input reads
    /// as `{}`, invalid JSON is [`Error::ToolInput`].
    fn finish_tool(&mut self) -> Result<Vec<StreamEvent>, Error> {
        let Some(PendingTool { id, name, input }) = self.tool.take() else {
            return Ok(Vec::new());
        };

        if input.is_empty() {
            let input = serde_json::Value::Object(serde_json::Map::new());
            return Ok(vec![StreamEvent::ToolUse(ToolUseCall { id, name, input })]);
        }

        match serde_json::from_str(&input) {
            Ok(input) => Ok(vec![StreamEvent::ToolUse(ToolUseCall { id, name, input })]),
            Err(source) => Err(Error::ToolInput { name, id, source }),
        }
    }
}

/// The usage of `message_start`: input and cache counters. Its
/// `output_tokens` is left out; `message_delta` carries the cumulative total.
fn start_usage(usage: WireUsage) -> StreamEvent {
    StreamEvent::Usage(Usage {
        input_tokens: usage.input_tokens,
        output_tokens: 0,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
    })
}

/// The usage of `message_delta`: the output count only.
fn delta_usage(usage: DeltaUsage) -> StreamEvent {
    StreamEvent::Usage(Usage {
        input_tokens: 0,
        output_tokens: usage.output_tokens,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
    })
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
        #[serde(default)]
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
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
    #[serde(other)]
    Other,
}

/// The `delta` of `message_delta`.
#[derive(Debug, Default, Deserialize)]
struct MessageDeltaBody {
    #[serde(default)]
    stop_reason: Option<WireStopReason>,
}

/// The `usage` of `message_delta`: the cumulative output count.
#[derive(Debug, Clone, Copy, Deserialize)]
struct DeltaUsage {
    #[serde(default)]
    output_tokens: u32,
}

#[cfg(test)]
mod tests;
