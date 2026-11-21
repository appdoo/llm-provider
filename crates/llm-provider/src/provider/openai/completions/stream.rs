//! Decoding a `/completions` stream into [`StreamEvent`]s: `data:` lines of
//! JSON chunks with a `text` per choice, `data: [DONE]` at the end.

use serde::Deserialize;

use crate::provider::api_error::EnvelopeError;
use crate::provider::error::Error;
use crate::provider::openai::stream::{ChatUsage, FinishReason};
use crate::provider::EventStream;
use crate::provider::{StopReason, StreamEvent};
use crate::transport::{decode_lines, sse_data, ByteStream, LineDecoder};

/// Decodes a response body into events. A body without bytes is
/// [`Error::EmptyResponse`]; the first error ends the stream. The first
/// finish reason is emitted as [`StreamEvent::Stop`] right before
/// [`StreamEvent::MessageStop`] at `[DONE]`. A body that ends without
/// `[DONE]` is complete when a finish reason came, else
/// [`Error::UnexpectedEnd`].
pub(crate) fn decode(bytes: ByteStream) -> EventStream {
    decode_lines(bytes, TextChunkDecoder::default())
}

/// Turns `data:` lines into events, keeping the first finish reason for
/// the end.
#[derive(Debug, Default)]
struct TextChunkDecoder {
    stop: Option<StopReason>,
    done: bool,
}

impl LineDecoder for TextChunkDecoder {
    /// The events one line stands for; lines other than `data:` stand for
    /// none. An error chunk is [`Error::Api`].
    fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error> {
        let Some(data) = sse_data(line) else {
            return Ok(Vec::new());
        };
        if data.trim() == "[DONE]" {
            return Ok(self.finish());
        }

        let chunk: TextChunk = serde_json::from_str(data)?;
        if let Some(error) = chunk.error {
            return Err(Error::Api(error.into()));
        }

        let mut events = Vec::new();
        if let Some(choice) = chunk.choices.into_iter().next() {
            if !choice.text.is_empty() {
                events.push(StreamEvent::ContentDelta(choice.text));
            }
            if let Some(reason) = choice.finish_reason {
                self.stop.get_or_insert(reason.into());
            }
        }
        events.extend(chunk.usage.map(|usage| StreamEvent::Usage(usage.into())));

        Ok(events)
    }

    /// Whether `[DONE]` was seen.
    fn done(&self) -> bool {
        self.done
    }

    /// [`Error::UnexpectedEnd`] without a finish reason, else the stop
    /// reason and [`StreamEvent::MessageStop`].
    fn at_end(&mut self) -> Result<Vec<StreamEvent>, Error> {
        if self.stop.is_none() {
            return Err(Error::UnexpectedEnd);
        }
        Ok(self.finish())
    }
}

impl TextChunkDecoder {
    /// At `[DONE]`, or at a body's end after a finish reason: the kept stop
    /// reason, then [`StreamEvent::MessageStop`].
    fn finish(&mut self) -> Vec<StreamEvent> {
        self.done = true;

        let mut events: Vec<StreamEvent> = self.stop.map(StreamEvent::Stop).into_iter().collect();
        events.push(StreamEvent::MessageStop);
        events
    }
}

/// One streamed chunk: an error, or choices and usage. Every other field is
/// ignored.
#[derive(Debug, Deserialize)]
struct TextChunk {
    #[serde(default)]
    error: Option<EnvelopeError>,
    #[serde(default)]
    choices: Vec<TextChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

/// One choice of a chunk; only the first is read.
#[derive(Debug, Deserialize)]
struct TextChoice {
    #[serde(default)]
    text: String,
    #[serde(default)]
    finish_reason: Option<FinishReason>,
}
