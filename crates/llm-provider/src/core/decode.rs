//! Decoders from a reply's bytes to events: the ones our providers use and
//! [`lines`], the building block for a line format of your own.

use super::ByteStream;
use crate::provider::{Error, EventStream, StreamEvent};
use crate::transport::{decode_lines, sse_data, LineDecoder};

/// Decodes an Anthropic Messages stream (server-sent events).
#[must_use = "a stream does nothing until polled"]
pub fn anthropic(bytes: ByteStream) -> EventStream {
    crate::provider::decode_anthropic(bytes)
}

/// Decodes an OpenAI Chat Completions stream (server-sent events).
#[must_use = "a stream does nothing until polled"]
pub fn openai_chat(bytes: ByteStream) -> EventStream {
    crate::provider::decode_openai_chat(bytes)
}

/// Decodes an OpenAI `/completions` stream (server-sent events).
#[must_use = "a stream does nothing until polled"]
pub fn openai_completions(bytes: ByteStream) -> EventStream {
    crate::provider::decode_openai_completions(bytes)
}

/// Decodes a line format: each line of `bytes` goes to `per_line`, which
/// returns the event it carries, if any, or an [`Error`]; a
/// `serde_json::Error` converts with `?`.
///
/// Server-sent events work as they are: the `data:` prefix is stripped,
/// `event:`, `id:`, `retry:` and comment lines and blank lines are skipped,
/// and `data: [DONE]` ends the stream. Newline-delimited JSON passes each
/// line whole. The stream ends with [`StreamEvent::MessageStop`]; a reply
/// with no bytes is [`Error::EmptyResponse`].
#[must_use = "a stream does nothing until polled"]
pub fn lines<F>(bytes: ByteStream, per_line: F) -> EventStream
where
    F: FnMut(&str) -> Result<Option<StreamEvent>, Error> + Send + 'static,
{
    decode_lines(
        bytes,
        PerLine {
            per_line,
            done: false,
        },
    )
}

/// [`lines`]' decoder: the caller's closure behind the line framing.
struct PerLine<F> {
    per_line: F,
    done: bool,
}

impl<F> LineDecoder for PerLine<F>
where
    F: FnMut(&str) -> Result<Option<StreamEvent>, Error>,
{
    fn feed_line(&mut self, line: &str) -> Result<Vec<StreamEvent>, Error> {
        let payload = sse_data(line).unwrap_or(line);
        if payload.trim() == "[DONE]" {
            self.done = true;
            return Ok(vec![StreamEvent::MessageStop]);
        }
        if payload.trim().is_empty() || is_sse_field(payload) {
            return Ok(Vec::new());
        }

        Ok((self.per_line)(payload)?.into_iter().collect())
    }

    fn done(&self) -> bool {
        self.done
    }

    fn at_end(&mut self) -> Result<Vec<StreamEvent>, Error> {
        Ok(vec![StreamEvent::MessageStop])
    }
}

/// True for an SSE line that carries no data: a comment or a field other
/// than `data`.
fn is_sse_field(line: &str) -> bool {
    line.starts_with(':')
        || ["event:", "id:", "retry:"]
            .iter()
            .any(|field| line.starts_with(field))
}
