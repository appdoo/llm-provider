//! Decoding a `/chat/completions` stream into [`StreamEvent`]s: `data:`
//! lines of JSON chunks, `data: [DONE]` at the end.

use serde::Deserialize;
use tokio_stream::StreamExt;

use crate::provider::api_error::EnvelopeError;
use crate::provider::completion::Usage;
use crate::provider::error::Error;
use crate::provider::openai::tool_calls::{ToolCallFragment, ToolCalls};
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent};
use crate::transport::{ByteStream, LineAccumulator};

/// Decodes a response body into events. A body without bytes is
/// [`Error::EmptyResponse`]; the first error ends the stream. The first
/// finish reason is emitted as [`StreamEvent::Stop`] right before
/// [`StreamEvent::MessageStop`] at `[DONE]`.
pub(crate) fn decode(mut bytes: ByteStream) -> EventStream {
    Box::pin(async_stream::try_stream! {
        let mut lines = LineAccumulator::new();
        let mut decoder = ChunkDecoder::default();
        let mut received = false;

        'body: while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            received |= !chunk.is_empty();
            lines.extend(&chunk);
            while let Some(line) = lines.next_line()? {
                for event in decoder.feed_line(&line)? {
                    yield event;
                }
                if decoder.done {
                    break 'body;
                }
            }
        }

        if !decoder.done {
            if !received {
                Err(Error::EmptyResponse)?;
            }
            lines.finish()?;
        }
    })
}

/// Turns `data:` lines into events.
#[derive(Debug, Default)]
struct ChunkDecoder {
    calls: ToolCalls,
    stop: Option<StopReason>,
    done: bool,
}

impl ChunkDecoder {
    /// The events one line stands for; lines other than `data:` stand for
    /// none. An error chunk is [`Error::Api`].
    fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error> {
        let Some(data) = line.strip_prefix("data: ") else {
            return Ok(Vec::new());
        };
        if data.trim() == "[DONE]" {
            return self.finish();
        }

        let chunk: Chunk = serde_json::from_str(data)?;
        if let Some(error) = chunk.error {
            return Err(Error::Api(error.into()));
        }

        let mut events = match chunk.choices.into_iter().next() {
            Some(choice) => self.apply(choice)?,
            None => Vec::new(),
        };
        events.extend(chunk.usage.map(|usage| StreamEvent::Usage(usage.into())));

        Ok(events)
    }

    /// The text of `choice`; its tool-call fragments are buffered, and its
    /// finish reason, the first one only, is kept and flushes the calls.
    fn apply(&mut self, choice: Choice) -> Result<Vec<StreamEvent>, Error> {
        let mut events = Vec::new();
        let delta = choice.delta.unwrap_or_default();

        if let Some(text) = delta.content.filter(|text| !text.is_empty()) {
            events.push(StreamEvent::ContentDelta(text));
        }
        for fragment in delta.tool_calls.unwrap_or_default() {
            self.calls.push(fragment)?;
        }

        if let Some(reason) = choice.finish_reason {
            self.stop.get_or_insert(reason.into());
            events.extend(self.flush_calls()?);
        }

        Ok(events)
    }

    /// At `[DONE]`: the calls not yet flushed, the stop reason, then
    /// [`StreamEvent::MessageStop`].
    fn finish(&mut self) -> Result<Vec<StreamEvent>, Error> {
        self.done = true;

        let mut events = self.flush_calls()?;
        events.extend(self.stop.map(StreamEvent::Stop));
        events.push(StreamEvent::MessageStop);

        Ok(events)
    }

    /// Every buffered call as [`StreamEvent::ToolUse`], once.
    fn flush_calls(&mut self) -> Result<Vec<StreamEvent>, Error> {
        let calls = self.calls.flush()?;
        Ok(calls.into_iter().map(StreamEvent::ToolUse).collect())
    }
}

/// One streamed chunk: an error, or choices and usage.
#[derive(Debug, Deserialize)]
struct Chunk {
    #[serde(default)]
    error: Option<EnvelopeError>,
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

/// One choice of a chunk; only the first is read.
#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Option<Delta>,
    #[serde(default)]
    finish_reason: Option<FinishReason>,
}

/// The new text and tool-call fragments of a choice.
#[derive(Debug, Default, Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallFragment>>,
}

/// A `finish_reason`; values this crate does not know read as
/// [`Unknown`](Self::Unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    #[serde(other)]
    Unknown,
}

impl From<FinishReason> for StopReason {
    fn from(reason: FinishReason) -> Self {
        match reason {
            FinishReason::Stop => StopReason::EndTurn,
            FinishReason::ToolCalls => StopReason::ToolUse,
            FinishReason::Length => StopReason::MaxTokens,
            FinishReason::ContentFilter => StopReason::Refusal,
            FinishReason::Unknown => StopReason::Unknown,
        }
    }
}

/// The `usage` of the final chunk; absent counters read as 0.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    prompt_tokens_details: Option<PromptTokensDetails>,
}

/// The breakdown of the prompt tokens.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(default)]
struct PromptTokensDetails {
    cached_tokens: u32,
}

impl From<ChatUsage> for Usage {
    /// Cached prompt tokens are cache reads; `input_tokens` is the
    /// uncached rest.
    fn from(usage: ChatUsage) -> Self {
        let cached = usage
            .prompt_tokens_details
            .unwrap_or_default()
            .cached_tokens;

        Usage {
            input_tokens: usage.prompt_tokens.saturating_sub(cached),
            output_tokens: usage.completion_tokens,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: cached,
        }
    }
}

#[cfg(test)]
mod tests;
